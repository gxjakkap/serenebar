//! Notification-area icons for the hidden-icons widget.
//!
//! Windows has no API to list other apps' tray icons. Apps register them with
//! Shell_NotifyIcon, which finds the taskbar with FindWindow("Shell_TrayWnd") and sends it a
//! WM_COPYDATA. While this widget is on, serenebar keeps a hidden window of that class in front
//! of Explorer's, so it receives those messages first. It keeps a copy of each icon and passes
//! every message on to Explorer unchanged, so the real tray, app bars and anything else talking
//! to the taskbar keep working. Turning the widget off removes the window.
use crate::system::wstr;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

/// NOTIFYICONDATAW as shell32 sends it to the taskbar: handles are always 32 bits wide.
#[repr(C)]
#[derive(Clone, Copy)]
struct IconData32 {
    cb_size: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback: u32,
    icon: u32,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: GUID,
    balloon_icon: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrayMessage {
    signature: u32, // 0x34753423
    message: u32,   // NIM_*
    data: IconData32,
}

const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_STATE: u32 = 0x8;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x1;

#[derive(Clone)]
pub struct TrayIcon {
    pub hwnd: isize, // the app window that gets clicks
    pub uid: u32,
    pub guid: Option<GUID>,
    pub callback: u32, // message the app wants clicks as
    pub version: u32,  // NOTIFYICON_VERSION_4 changes how clicks are reported
    pub icon: isize,   // our own copy of the HICON
    pub stamp: u64,    // changes whenever the icon does
    pub tip: String,
    pub hidden: bool,
}

impl TrayIcon {
    pub fn key(&self) -> String {
        match self.guid {
            Some(g) => format!("{g:?}"),
            None => format!("{}:{}", self.hwnd, self.uid),
        }
    }
}

static ICONS: Mutex<Vec<TrayIcon>> = Mutex::new(Vec::new());
static HOST: AtomicIsize = AtomicIsize::new(0);
static MAIN: AtomicIsize = AtomicIsize::new(0);
static STAMP: AtomicU64 = AtomicU64::new(1);

/// 32-bit handles from other processes are sign-extended, as WOW64 does.
fn handle(v: u32) -> isize {
    v as i32 as isize
}

pub fn icons() -> Vec<TrayIcon> {
    ICONS
        .lock()
        .unwrap()
        .iter()
        .filter(|i| !i.hidden && i.icon != 0)
        .cloned()
        .collect()
}

fn record(m: &TrayMessage) {
    let d = &m.data;
    let guid = (d.flags & NIF_GUID != 0).then_some(d.guid);
    let (hwnd, uid) = (handle(d.hwnd), d.uid);
    let mut icons = ICONS.lock().unwrap();
    let pos = icons.iter().position(|i| match guid {
        Some(g) => i.guid == Some(g),
        None => i.guid.is_none() && i.hwnd == hwnd && i.uid == uid,
    });
    match m.message {
        0 | 1 => {
            // NIM_ADD, NIM_MODIFY
            let i = match pos {
                Some(p) => p,
                None => {
                    icons.push(TrayIcon {
                        hwnd,
                        uid,
                        guid,
                        callback: 0,
                        version: 0,
                        icon: 0,
                        stamp: 0,
                        tip: String::new(),
                        hidden: false,
                    });
                    icons.len() - 1
                }
            };
            let it = &mut icons[i];
            if d.flags & NIF_MESSAGE != 0 {
                it.callback = d.callback;
            }
            if d.flags & NIF_TIP != 0 {
                it.tip = wstr(&d.tip);
            }
            if d.flags & NIF_STATE != 0 && d.state_mask & NIS_HIDDEN != 0 {
                it.hidden = d.state & NIS_HIDDEN != 0;
            }
            if d.flags & NIF_ICON != 0 {
                // the app may destroy its icon right after, so keep a copy of our own
                let copy =
                    unsafe { CopyIcon(HICON(handle(d.icon) as _)) }.map_or(0, |h| h.0 as isize);
                if it.icon != 0 {
                    unsafe {
                        let _ = DestroyIcon(HICON(it.icon as _));
                    }
                }
                it.icon = copy;
                it.stamp = STAMP.fetch_add(1, Ordering::Relaxed);
            }
        }
        2 => {
            if let Some(p) = pos {
                // NIM_DELETE
                let gone = icons.remove(p);
                if gone.icon != 0 {
                    unsafe {
                        let _ = DestroyIcon(HICON(gone.icon as _));
                    }
                }
            }
        }
        4 => {
            if let Some(p) = pos {
                icons[p].version = d.version;
            }
        } // NIM_SETVERSION
        _ => {}
    }
    drop(icons);
    crate::system::notify(MAIN.load(Ordering::SeqCst));
}

/// Puts our window just in front of Explorer's tray, over the same spot, so lookups that
/// measure the taskbar still get its real position.
fn stay_in_front(host: HWND) {
    let Some(real) = crate::taskbar::explorer_tray() else {
        return;
    };
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(real, &mut r);
        let _ = SetWindowPos(
            host,
            HWND_TOPMOST,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOACTIVATE,
        );
    }
}

extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COPYDATA => {
                let cds = &*(lp.0 as *const COPYDATASTRUCT);
                if cds.dwData == 1 && !cds.lpData.is_null() {
                    // copy what was sent into a zeroed message, older apps send less
                    let mut m: TrayMessage = std::mem::zeroed();
                    let n = (cds.cbData as usize).min(std::mem::size_of::<TrayMessage>());
                    std::ptr::copy_nonoverlapping(
                        cds.lpData as *const u8,
                        &mut m as *mut _ as *mut u8,
                        n,
                    );
                    if m.signature == 0x34753423 {
                        record(&m);
                    }
                }
                match crate::taskbar::explorer_tray() {
                    Some(real) => SendMessageW(real, msg, wp, lp),
                    None => LRESULT(1),
                }
            }
            WM_TIMER => {
                stay_in_front(hwnd);
                // drop icons whose app went away without removing them
                let mut icons = ICONS.lock().unwrap();
                icons.retain(|i| {
                    let alive = IsWindow(HWND(i.hwnd as _)).as_bool();
                    if !alive && i.icon != 0 {
                        let _ = DestroyIcon(HICON(i.icon as _));
                    }
                    alive
                });
                LRESULT(0)
            }
            // the taskbar's own commands go on to Explorer. Registered messages (0xC000 and up)
            // are broadcasts like TaskbarCreated, meant for every window, so they stay here.
            m if m == WM_COMMAND || (WM_USER..0xC000).contains(&m) => {
                match crate::taskbar::explorer_tray() {
                    Some(real) if InSendMessage().as_bool() => SendMessageW(real, msg, wp, lp),
                    Some(real) => {
                        let _ = PostMessageW(real, msg, wp, lp);
                        LRESULT(0)
                    }
                    None => LRESULT(0),
                }
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Starts receiving tray icons. Apps re-register theirs when told the taskbar was recreated.
pub fn start(main: HWND) {
    if HOST.load(Ordering::SeqCst) != 0 {
        return;
    }
    MAIN.store(main.0 as isize, Ordering::SeqCst);
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            hInstance: hinst.into(),
            lpszClassName: w!("Shell_TrayWnd"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let Ok(host) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("Shell_TrayWnd"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            hinst,
            None,
        ) else {
            return;
        };
        HOST.store(host.0 as isize, Ordering::SeqCst);
        stay_in_front(host);
        SetTimer(host, 1, 1000, None);
        let created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let _ = PostMessageW(HWND_BROADCAST, created, WPARAM(0), LPARAM(0));
    }
}

pub fn stop() {
    let host = HOST.swap(0, Ordering::SeqCst);
    if host == 0 {
        return;
    }
    unsafe {
        let _ = DestroyWindow(HWND(host as _));
    }
    for i in ICONS.lock().unwrap().drain(..) {
        if i.icon != 0 {
            unsafe {
                let _ = DestroyIcon(HICON(i.icon as _));
            }
        }
    }
}

/// Clicks an icon the way the taskbar does. `at` is where its menu should open.
pub fn click(icon: &TrayIcon, right: bool, at: POINT) {
    if icon.callback == 0 {
        return;
    }
    let hwnd = HWND(icon.hwnd as _);
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid); // so its menu or window can come to the front
        let send = |event: u32| {
            let (wp, lp) = if icon.version >= 4 {
                (
                    WPARAM(((at.y as u16 as usize) << 16) | at.x as u16 as usize),
                    LPARAM(((icon.uid as isize) << 16) | event as isize),
                )
            } else {
                (WPARAM(icon.uid as usize), LPARAM(event as isize))
            };
            let _ = PostMessageW(hwnd, icon.callback, wp, lp);
        };
        if right {
            send(WM_RBUTTONDOWN);
            send(WM_RBUTTONUP);
            if icon.version >= 4 {
                send(WM_CONTEXTMENU);
            }
        } else {
            send(WM_LBUTTONDOWN);
            send(WM_LBUTTONUP);
            if icon.version >= 4 {
                send(WM_USER);
            } // NIN_SELECT
        }
    }
}
