//! Small pieces of Windows: CPU, memory, battery, keyboard layout, keys, the clock, startup.
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Power::*;
use windows::Win32::System::SystemInformation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Posted to the bar by the worker threads and the tray host when they have something new.
pub const WM_DATA: u32 = WM_APP + 2;

pub fn notify(h: isize) {
    unsafe {
        let _ = PostMessageW(HWND(h as _), WM_DATA, WPARAM(0), LPARAM(0));
    }
}

pub fn cpu(prev: &mut (u64, u64)) -> f32 {
    let (mut idle, mut kernel, mut user) = Default::default();
    unsafe {
        let _ = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
    }
    let ft = |t: FILETIME| (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64;
    let (i, t) = (ft(idle), ft(kernel) + ft(user)); // kernel time includes idle
    let (di, dt) = (i.saturating_sub(prev.0), t.saturating_sub(prev.1));
    *prev = (i, t);
    if dt == 0 {
        0.0
    } else {
        1.0 - di as f32 / dt as f32
    }
}

pub fn mem() -> f32 {
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GlobalMemoryStatusEx(&mut m);
    }
    m.dwMemoryLoad as f32 / 100.0
}

/// Charge fraction and whether it's charging. None on machines without a battery.
pub fn battery() -> Option<(f32, bool)> {
    let mut p = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut p).ok()? };
    if p.BatteryFlag & 128 != 0 || p.BatteryLifePercent > 100 {
        return None;
    }
    Some((p.BatteryLifePercent as f32 / 100.0, p.ACLineStatus == 1))
}

pub fn wstr(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

pub fn class_of(h: HWND) -> String {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut class) } as usize;
    String::from_utf16_lossy(&class[..n])
}

/// Presses the keys in order and releases them in reverse, like a shortcut.
pub fn press_keys(keys: &[VIRTUAL_KEY]) {
    let key = |vk, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&k| key(k, KEYBD_EVENT_FLAGS(0)))
        .chain(keys.iter().rev().map(|&k| key(k, KEYEVENTF_KEYUP)))
        .collect();
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Keyboard layout of the window in front, as Windows labels it: ("ENG", "US").
pub fn input_language() -> Option<(String, String)> {
    use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName};
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let lang = GetKeyboardLayout(thread).0 as usize as u32 & 0xFFFF;
        let mut name = [0u16; 85];
        if LCIDToLocaleName(lang, Some(&mut name), 0) == 0 {
            return None;
        }
        let info = |kind: u32| {
            let mut out = [0u16; 16];
            let n = GetLocaleInfoEx(PCWSTR(name.as_ptr()), kind, Some(&mut out));
            (n > 0).then(|| wstr(&out).to_uppercase())
        };
        // LOCALE_SISO639LANGNAME2 ("eng") and LOCALE_SISO3166CTRYNAME ("US")
        Some((info(0x67)?, info(0x5A).unwrap_or_default()))
    }
}

pub fn open(what: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(what),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

pub fn local_time() -> crate::dates::LocalTime {
    let t = unsafe { GetLocalTime() };
    crate::dates::LocalTime {
        year: t.wYear as i32,
        month: t.wMonth as u32,
        day: t.wDay as u32,
        hour: t.wHour as u32,
        minute: t.wMinute as u32,
        second: t.wSecond as u32,
    }
}

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

pub fn startup_enabled() -> bool {
    use windows::Win32::System::Registry::*;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            w!("serenebar"),
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
        .is_ok()
    }
}

pub fn set_startup(on: bool) {
    use windows::Win32::System::Registry::*;
    unsafe {
        if on {
            let exe = std::env::current_exe()
                .map(|p| format!("\"{}\"", p.display()))
                .unwrap_or_default();
            let data: Vec<u16> = exe.encode_utf16().chain([0]).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                w!("serenebar"),
                REG_SZ.0,
                Some(data.as_ptr() as _),
                (data.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("serenebar"));
        }
    }
}

const PERSONALIZE: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");

/// Whether apps are in light mode. None when Windows hasn't recorded a choice.
pub fn light_theme() -> Option<bool> {
    use windows::Win32::System::Registry::*;
    let mut v = 0u32;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PERSONALIZE,
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    Some(v != 0)
}

/// Switches apps and the taskbar between light and dark, as the Settings app does.
pub fn toggle_theme() {
    use windows::Win32::System::Registry::*;
    let light = u32::from(!light_theme().unwrap_or(true));
    for name in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
        unsafe {
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PERSONALIZE,
                name,
                REG_DWORD.0,
                Some(&light as *const u32 as _),
                4,
            );
        }
    }
    // a hung window would hold the broadcast up, so it goes out on its own thread
    std::thread::spawn(|| unsafe {
        let topic = w!("ImmersiveColorSet");
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(topic.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        );
    });
}
