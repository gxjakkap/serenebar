//! Dock apps: which windows and pins belong together, and in what order they sit.

/// What the taskbar groups windows and pins by: the AppUserModelID, else the exe path.
/// Both lower-case, either may be empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppId {
    pub aumid: String,
    pub exe: String,
    /// Squirrel apps (Discord, Slack...) pin `Update.exe --processStart App.exe` and run
    /// `<dir>\app-1.2.3\App.exe`: the install dir and the exe it starts.
    pub launcher: Option<(String, String)>,
}

impl AppId {
    pub fn new(aumid: &str, exe: &str) -> AppId {
        let exe = exe.to_lowercase();
        let mut aumid = aumid.to_lowercase();
        // File Explorer windows carry no explicit id, but its pin does
        if aumid.is_empty() && exe.ends_with("\\explorer.exe") {
            aumid = "microsoft.windows.explorer".into();
        }
        AppId {
            aumid,
            exe,
            launcher: None,
        }
    }

    /// Reads a shortcut's arguments for `--processStart App.exe`.
    pub fn with_args(mut self, args: &str) -> AppId {
        let mut words = args.split_whitespace();
        if words.any(|w| w.eq_ignore_ascii_case("--processStart")) {
            if let (Some(name), Some(dir)) = (
                words.next(),
                self.exe.rsplit_once('\\').map(|d| d.0.to_string()),
            ) {
                self.launcher = Some((dir + "\\", name.trim_matches('"').to_lowercase()));
            }
        }
        self
    }

    fn started_by(&self, o: &AppId) -> bool {
        self.launcher.as_ref().is_some_and(|(dir, name)| {
            o.exe.starts_with(dir.as_str()) && o.exe.ends_with(&format!("\\{name}"))
        })
    }

    pub fn matches(&self, o: &AppId) -> bool {
        (!self.aumid.is_empty() && self.aumid == o.aumid)
            || (!self.exe.is_empty() && self.exe == o.exe)
            || self.started_by(o)
            || o.started_by(self)
    }

    pub fn key(&self) -> &str {
        if self.aumid.is_empty() {
            &self.exe
        } else {
            &self.aumid
        }
    }
}

/// Keeps keys seen before in their place and appends new ones, so raising a window doesn't
/// reshuffle the dock. Keys no longer present are dropped.
pub fn stable_order(order: &mut Vec<String>, present: &[&str]) {
    order.retain(|k| present.contains(&k.as_str()));
    for p in present {
        if !order.iter().any(|k| k == p) {
            order.push(p.to_string());
        }
    }
}

/// Unread count an app puts in its window title, like "(3) Discord" or "Telegram (5)".
pub fn parse_badge(title: &str) -> u32 {
    let t = title.trim();
    let first = t
        .strip_prefix('(')
        .and_then(|r| r.split_once(')'))
        .map(|x| x.0);
    let last = t
        .strip_suffix(')')
        .and_then(|r| r.rsplit_once('('))
        .map(|x| x.1);
    [first, last]
        .into_iter()
        .flatten()
        .find_map(|n| n.trim().trim_end_matches('+').parse().ok())
        .unwrap_or(0)
}

/// True when everything drawn in a square 32-bit BGRA image of width `w` fits in a quarter
/// of it. The shell returns this when an exe has no 256 px icon: a 32 or 48 px icon centred
/// on an empty 256 px canvas.
pub fn mostly_padding(bgra: &[u8], w: usize) -> bool {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, px) in bgra.as_chunks::<4>().0.iter().enumerate() {
        if px[3] != 0 {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    x0 == usize::MAX || (x1 - x0 + 1).max(y1 - y0 + 1) * 4 <= w
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use crate::system::{class_of, wstr};

    use crate::apps::{stable_order, AppId};
    use windows::core::*;

    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Dwm::*;
    use windows::Win32::Graphics::Gdi::{DeleteObject, GetObjectW, BITMAP, HBITMAP};
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::*;

    use windows::Win32::System::Threading::*;

    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreForWindow, PROPERTYKEY,
    };
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    pub struct Pinned {
        pub launch: String,
        pub id: AppId,
    }

    pub struct AppItem {
        pub id: AppId,
        pub launch: String,
        pub icon: Vec<String>, // shell parsing names to take the icon from, best first
        pub windows: Vec<HWND>, // in a fixed order, so clicking cycles through all of them
        pub top: HWND,         // the one nearest the front
        pub badge: u32,        // unread count read from the window titles
        pub pin: Option<usize>, // its place in the dock's pin list
    }

    impl AppItem {
        /// Icon cache and dock-order key. Pins with neither an id nor an exe fall back to their shortcut.
        pub fn key(&self) -> &str {
            let k = self.id.key();
            if k.is_empty() {
                &self.launch
            } else {
                k
            }
        }
    }

    /// System.AppUserModel.ID, the id the taskbar groups by.
    const PKEY_AUMID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
        pid: 5,
    };

    fn aumid_of(store: Result<IPropertyStore>) -> String {
        store
            .and_then(|s| unsafe { s.GetValue(&PKEY_AUMID) })
            .map(|v| v.to_string())
            .unwrap_or_default()
    }

    /// Shortcuts pinned to the Windows taskbar, which the dock starts out with.
    pub fn taskbar_pins() -> Vec<String> {
        let dir = std::env::var("APPDATA").unwrap_or_default()
            + r"\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar";
        let mut lnks: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        lnks.sort();
        lnks
    }

    const APPS_FOLDER: &str = r"shell:AppsFolder\";

    /// The dock's pins: shortcuts, exe paths, or Apps folder entries for Store apps.
    pub fn pinned(entries: &[String]) -> Vec<Pinned> {
        entries
            .iter()
            .map(|e| {
                let id = if e.to_lowercase().ends_with(".lnk") {
                    let (exe, aumid, args) = resolve_lnk(e).unwrap_or_default();
                    AppId::new(&aumid, &exe).with_args(&args)
                } else if let Some(aumid) = e.strip_prefix(APPS_FOLDER) {
                    AppId::new(aumid, "")
                } else {
                    AppId::new("", e)
                };
                Pinned {
                    launch: e.clone(),
                    id,
                }
            })
            .collect()
    }

    /// What to store when pinning a running app: its Apps folder entry when Windows knows it
    /// (Store apps can only be started that way), otherwise its exe.
    pub fn pin_entry(item: &AppItem) -> String {
        let parses = |n: &str| unsafe {
            SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(n), None).is_ok()
        };
        item.icon
            .iter()
            .find(|n| n.starts_with(APPS_FOLDER) && parses(n))
            .cloned()
            .unwrap_or_else(|| {
                item.icon
                    .last()
                    .cloned()
                    .unwrap_or_else(|| item.launch.clone())
            })
    }

    /// A shortcut's target exe, AppUserModelID and arguments. Store app pins have no exe.
    fn resolve_lnk(path: &str) -> Result<(String, String, String)> {
        unsafe {
            let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            sl.cast::<IPersistFile>()?
                .Load(&HSTRING::from(path), STGM_READ)?;
            let mut target = [0u16; 520];
            let _ = sl.GetPath(&mut target, std::ptr::null_mut::<WIN32_FIND_DATAW>(), 0);
            let mut args = [0u16; 1024];
            let _ = sl.GetArguments(&mut args);
            Ok((wstr(&target), aumid_of(sl.cast()), wstr(&args)))
        }
    }

    /// Taskbar-style top-level windows with their exe path, front to back.
    fn windows(own: HWND) -> Vec<(HWND, String)> {
        unsafe extern "system" fn push(h: HWND, l: LPARAM) -> BOOL {
            (*(l.0 as *mut Vec<HWND>)).push(h);
            TRUE
        }
        let mut all: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(push), LPARAM(&mut all as *mut _ as isize));
            all.into_iter()
                .filter(|&h| {
                    if h == own || !IsWindowVisible(h).as_bool() || GetWindowTextLengthW(h) == 0 {
                        return false;
                    }
                    if GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
                        return false;
                    }
                    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
                    if ex & WS_EX_TOOLWINDOW.0 != 0 {
                        return false;
                    }
                    let mut cloaked = 0u32;
                    let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as _, 4);
                    if cloaked != 0 {
                        return false;
                    }
                    let class = class_of(h);
                    !["Progman", "WorkerW"].contains(&class.as_str())
                        && !crate::taskbar::TASKBAR_CLASSES.contains(&class.as_str())
                })
                .filter_map(|h| Some((h, exe_of(h)?)))
                .collect()
        }
    }

    fn title(h: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
        String::from_utf16_lossy(&buf[..n])
    }

    fn exe_of(h: HWND) -> Option<String> {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        exe_of_pid(pid)
    }

    /// The full path of a process's program.
    pub fn exe_of_pid(pid: u32) -> Option<String> {
        unsafe {
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                p,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(p);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Pinned apps first along with their windows, then every other app with a window, in the
    /// order they first appeared. Windows and pins match by AppUserModelID or exe, like the taskbar.
    pub fn apps(
        pinned: &[Pinned],
        own: HWND,
        order: &mut Vec<String>,
        saved: &[String],
    ) -> Vec<AppItem> {
        let mut items: Vec<AppItem> = pinned
            .iter()
            .enumerate()
            .map(|(n, p)| AppItem {
                id: p.id.clone(),
                launch: p.launch.clone(),
                icon: vec![p.launch.clone()],
                windows: vec![],
                top: HWND::default(),
                badge: 0,
                pin: Some(n),
            })
            .collect();
        let pins = items.len();
        for (h, exe) in windows(own) {
            let aumid = aumid_of(unsafe { SHGetPropertyStoreForWindow(h) });
            let id = AppId::new(&aumid, &exe);
            let badge = crate::apps::parse_badge(&title(h));
            match items.iter_mut().find(|i| i.id.matches(&id)) {
                Some(i) => {
                    if i.windows.is_empty() {
                        i.top = h;
                    }
                    i.windows.push(h);
                    i.badge = i.badge.max(badge);
                }
                None => {
                    // Store and packaged apps only have a usable icon and launcher in the Apps folder
                    let apps_folder =
                        (!id.aumid.is_empty()).then(|| format!("{APPS_FOLDER}{aumid}"));
                    let launch = apps_folder.clone().unwrap_or_else(|| exe.clone());
                    let icon = apps_folder.into_iter().chain([exe]).collect();
                    items.push(AppItem {
                        id,
                        launch,
                        icon,
                        windows: vec![h],
                        top: h,
                        badge,
                        pin: None,
                    });
                }
            }
        }
        let keys: Vec<&str> = items[pins..].iter().map(|i| i.key()).collect();
        stable_order(order, &keys);
        items[pins..].sort_by_key(|i| order.iter().position(|k| k == i.key()));
        for i in &mut items {
            i.windows.sort_by_key(|h| h.0 as usize);
        }
        // the order the user dragged the dock into wins; anything new goes after it
        items.sort_by_key(|i| {
            saved
                .iter()
                .position(|k| k == i.key())
                .unwrap_or(usize::MAX)
        });
        items
    }

    /// The name Explorer shows for the app: the shortcut's or Start menu entry's name.
    pub fn app_name(item: &AppItem) -> String {
        let shell = item.icon.iter().find_map(|n| unsafe {
            let si: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(n), None).ok()?;
            let p = si.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
            let name = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as _));
            name
        });
        let name =
            shell.unwrap_or_else(|| item.launch.rsplit('\\').next().unwrap_or("").to_string());
        name.trim_end_matches(".lnk")
            .trim_end_matches(".exe")
            .to_string()
    }

    /// System.FileDescription: the name a program gives itself, like "Google Chrome".
    const PKEY_FILE_DESCRIPTION: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x0CEF7D53_FA64_11D1_A203_0000F81FEDEE),
        pid: 3,
    };

    /// What the volume mixer calls a program: its description, else its file name.
    pub fn exe_name(path: &str) -> String {
        let desc = unsafe {
            SHCreateItemFromParsingName::<_, _, IShellItem2>(&HSTRING::from(path), None)
                .and_then(|si| si.GetString(&PKEY_FILE_DESCRIPTION))
                .ok()
                .and_then(|p| {
                    let s = p.to_string().ok();
                    CoTaskMemFree(Some(p.0 as _));
                    s
                })
        };
        desc.filter(|d| !d.trim().is_empty()).unwrap_or_else(|| {
            let file = path.rsplit('\\').next().unwrap_or(path);
            file.trim_end_matches(".exe").to_string()
        })
    }

    /// Asks every window of the app to close, like "Close all windows" on the taskbar.
    /// Apps that live in the notification area keep running there.
    pub fn quit_app(item: &AppItem) {
        for &h in &item.windows {
            unsafe {
                let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// The icon Explorer shows for a shell item, at least `px` wide. The bitmap is 32-bit with
    /// premultiplied alpha.
    pub fn app_image(names: &[String], px: u32) -> Option<HBITMAP> {
        // Icons come in fixed sizes (16, 32, 48, 256). At any other size the shell resizes with
        // no smoothing, which leaves jagged edges, so take the 256 one and let Direct2D's cubic
        // filter shrink it when drawing.
        // An exe with no 256 one comes back as its small icon on an empty 256 canvas, so ask
        // again at the tile size.
        names.iter().find_map(|n| unsafe {
            let f: IShellItemImageFactory =
                SHCreateItemFromParsingName(&HSTRING::from(n), None).ok()?;
            let get = |s: u32| {
                let s = s as i32;
                f.GetImage(SIZE { cx: s, cy: s }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
                    .ok()
            };
            let h = get(px.max(256))?;
            if px < 256 && padded(h) {
                let _ = DeleteObject(h);
                return get(px);
            }
            Some(h)
        })
    }

    unsafe fn padded(h: HBITMAP) -> bool {
        let mut bm = BITMAP::default();
        let ok = GetObjectW(
            h,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        ) != 0;
        if !ok || bm.bmBits.is_null() || bm.bmBitsPixel != 32 {
            return false;
        }
        let bits = std::slice::from_raw_parts(
            bm.bmBits as *const u8,
            (bm.bmWidthBytes * bm.bmHeight) as usize,
        );
        crate::apps::mostly_padding(bits, bm.bmWidth as usize)
    }

    /// Behaves like a taskbar click. It launches the app, focuses its window, cycles
    /// through several windows, or minimises the one already in front.
    pub fn activate(item: &AppItem) {
        unsafe {
            if item.windows.is_empty() {
                ShellExecuteW(
                    None,
                    w!("open"),
                    &HSTRING::from(&item.launch),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
                return;
            }
            let fg = GetForegroundWindow();
            let target = match item.windows.iter().position(|&w| w == fg) {
                Some(i) if item.windows.len() > 1 => item.windows[(i + 1) % item.windows.len()],
                Some(_) => {
                    let _ = ShowWindow(fg, SW_MINIMIZE);
                    return;
                }
                None => item.top,
            };
            if IsIconic(target).as_bool() {
                let _ = ShowWindow(target, SW_RESTORE);
            }
            let _ = SetForegroundWindow(target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_group_with_their_pins() {
        // Discord sets its id process-wide, which other apps can't read, so its window has no id.
        // The pin's arguments name the exe it starts.
        let pin = AppId::new(
            "com.squirrel.Discord.Discord",
            r"C:\Users\me\AppData\Local\Discord\Update.exe",
        )
        .with_args("--processStart Discord.exe");
        assert!(pin.matches(&AppId::new(
            "",
            r"C:\Users\me\AppData\Local\Discord\app-1.0.9\Discord.exe"
        )));
        assert!(!pin.matches(&AppId::new(
            "",
            r"C:\Users\me\AppData\Local\Slack\app-4.1\Discord.exe"
        )));
        // windows that do carry the id match on it
        let pin = AppId::new(
            "com.squirrel.Discord.Discord",
            r"C:\Users\me\AppData\Local\Discord\Update.exe",
        );
        let win = AppId::new(
            "com.squirrel.Discord.Discord",
            r"C:\Users\me\AppData\Local\Discord\app-1.0.9\Discord.exe",
        );
        assert!(pin.matches(&win));
        // the File Explorer pin has an id but no exe, its windows have an exe but no id
        let pin = AppId::new("Microsoft.Windows.Explorer", "");
        assert!(pin.matches(&AppId::new("", r"C:\Windows\explorer.exe")));
        // plain pins match on the exe
        assert!(AppId::new("", r"C:\Apps\Code.exe").matches(&AppId::new("", r"c:\apps\code.exe")));
        assert!(!AppId::new("", "").matches(&AppId::new("", "")));
        assert_eq!(AppId::new("", r"C:\A.exe").key(), r"c:\a.exe");
    }

    #[test]
    fn dock_order_survives_raising_a_window() {
        let mut order = vec![];
        stable_order(&mut order, &["code", "chrome", "zed"]);
        stable_order(&mut order, &["zed", "code", "chrome"]); // z-order changed after a click
        assert_eq!(order, ["code", "chrome", "zed"]);
        stable_order(&mut order, &["chrome", "zed", "t3"]); // code closed, t3 opened
        assert_eq!(order, ["chrome", "zed", "t3"]);
    }

    #[test]
    fn badges_from_titles() {
        assert_eq!(parse_badge("(2) Discord | #general"), 2);
        assert_eq!(parse_badge("Telegram (5)"), 5);
        assert_eq!(parse_badge("Notepad (Not Responding)"), 0);
        assert_eq!(parse_badge("Visual Studio Code"), 0);
        assert_eq!(parse_badge("(99+) Slack"), 99);
    }

    #[test]
    fn small_icon_on_a_big_canvas() {
        let canvas = |side: usize| {
            let mut b = vec![0u8; 256 * 256 * 4];
            let o = (256 - side) / 2;
            for y in o..o + side {
                for x in o..o + side {
                    b[(y * 256 + x) * 4 + 3] = 255;
                }
            }
            b
        };
        assert!(mostly_padding(&canvas(48), 256));
        assert!(!mostly_padding(&canvas(200), 256));
        assert!(mostly_padding(&vec![0u8; 256 * 256 * 4], 256));
    }
}
