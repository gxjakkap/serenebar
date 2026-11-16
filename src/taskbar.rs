//! Windows' own taskbar: hidden or auto-hidden while the dock runs, and where Start opens.
use crate::system::class_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Window classes of Explorer's main taskbar and the ones on other monitors.
pub const TASKBAR_CLASSES: [&str; 2] = ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

static TASKBAR_STATE: AtomicIsize = AtomicIsize::new(-1); // ABM_GETSTATE from before we changed it

/// Explorer's taskbars, main one first. Windows from this process are skipped: the tray
/// widget puts its own Shell_TrayWnd in front of Explorer's.
fn trays() -> Vec<HWND> {
    let mut v = vec![];
    let own = unsafe { GetCurrentProcessId() };
    for class in TASKBAR_CLASSES {
        let mut after = HWND::default();
        while let Ok(h) = unsafe { FindWindowExW(None, after, &HSTRING::from(class), None) } {
            if h.is_invalid() {
                break;
            }
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
            if pid != own {
                v.push(h);
            }
            after = h;
        }
    }
    v
}

pub fn explorer_tray() -> Option<HWND> {
    trays().into_iter().next()
}

static TASKBAR_HIDDEN: AtomicBool = AtomicBool::new(false);

static TRAY_HOOK: std::sync::Once = std::sync::Once::new();

/// Explorer shows a hidden taskbar again on its own: on an edge hover in auto-hide mode, for
/// Start, after a display change. While "hidden" is chosen, hide it the moment that happens.
unsafe extern "system" fn tray_shown(
    _: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _: u32,
    h: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    if object == OBJID_WINDOW.0
        && TASKBAR_HIDDEN.load(Ordering::SeqCst)
        && TASKBAR_CLASSES.contains(&class_of(h).as_str())
    {
        let _ = ShowWindow(h, SW_HIDE);
    }
}

/// Sets the Windows taskbar to auto-hide while we run, or hides it outright.
pub fn taskbar_take(hide: bool) {
    TASKBAR_HIDDEN.store(hide, Ordering::SeqCst);
    TRAY_HOOK.call_once(|| unsafe {
        use windows::Win32::UI::Accessibility::SetWinEventHook;
        SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            None,
            Some(tray_shown),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
    });
    let Some(tray) = explorer_tray() else { return };
    unsafe {
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: tray,
            ..Default::default()
        };
        let st = SHAppBarMessage(ABM_GETSTATE, &mut abd) as isize;
        let _ = TASKBAR_STATE.compare_exchange(-1, st, Ordering::SeqCst, Ordering::SeqCst);
        abd.lParam = LPARAM(ABS_AUTOHIDE as isize);
        SHAppBarMessage(ABM_SETSTATE, &mut abd);
        for t in trays() {
            let _ = ShowWindow(t, if hide { SW_HIDE } else { SW_SHOWNA });
        }
    }
}

const ADVANCED: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced");

static START_ALIGN: AtomicIsize = AtomicIsize::new(-1); // TaskbarAl from before we changed it

fn taskbar_al() -> u32 {
    use windows::Win32::System::Registry::*;
    let (mut v, mut n) = (1u32, 4u32); // Windows 11 centres by default
    let _ = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            ADVANCED,
            w!("TaskbarAl"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut _ as _),
            Some(&mut n),
        )
    };
    v
}

fn set_taskbar_al(v: u32) {
    use windows::Win32::System::Registry::*;
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            ADVANCED,
            w!("TaskbarAl"),
            REG_DWORD.0,
            Some(&v as *const _ as _),
            4,
        );
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("TraySettings").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            200,
            None,
        );
    }
}

/// Windows 11 opens Start either centred or at the left, following the taskbar alignment.
/// Pick the one closer to our Start tile. The original setting comes back on quit.
pub fn align_start(left: bool) {
    let now = taskbar_al();
    let _ = START_ALIGN.compare_exchange(-1, now as isize, Ordering::SeqCst, Ordering::SeqCst);
    let want = if left { 0 } else { 1 };
    if now != want {
        set_taskbar_al(want);
    }
}

pub fn taskbar_restore() {
    TASKBAR_HIDDEN.store(false, Ordering::SeqCst);
    let al = START_ALIGN.swap(-1, Ordering::SeqCst);
    if al >= 0 && taskbar_al() != al as u32 {
        set_taskbar_al(al as u32);
    }
    let st = TASKBAR_STATE.swap(-1, Ordering::SeqCst);
    if st < 0 {
        return;
    }
    unsafe {
        for t in trays() {
            let _ = ShowWindow(t, SW_SHOWNA);
        }
        if let Some(tray) = explorer_tray() {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: tray,
                lParam: LPARAM(st),
                ..Default::default()
            };
            SHAppBarMessage(ABM_SETSTATE, &mut abd);
        }
    }
}
