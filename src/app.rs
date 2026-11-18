use crate::apps::{self, Pinned};
use crate::controls::{self, Cmd};
use crate::feeds::{self, FeedConfig};
use crate::settings::{widgets_from_rows, Settings, Size, Style, Widget};
use crate::ui::{self, Action, Panel};
use crate::{dock, gfx, google, panel, prefs, system, taskbar, tray, update, weather};
use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::time::Instant;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_APPBAR: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_TICK: usize = 1;
const TIMER_GLASS: usize = 2; // polled at 30 Hz, only redraw when the desktop behind the bar changed
const TIMER_ANIM: usize = 3; // about 60 fps, only while something moves
const TIMER_SUGGEST: usize = 4; // city lookups wait for a pause in typing
const OPEN_SECS: f32 = 0.18; // panels and settings fade and rise in this long

/// Opacity, scale and drop (DIPs) of a window `t` after it opened.
fn opening(t: Instant) -> (f32, f32, f32) {
    let x = (t.elapsed().as_secs_f32() / OPEN_SECS).clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - x).powi(3);
    (e, 0.96 + 0.04 * e, 8.0 * (1.0 - e))
}

fn still_opening(t: Instant) -> bool {
    t.elapsed().as_secs_f32() < OPEN_SECS + 0.05
}

/// Coordinates packed in a mouse message.
fn xy(lp: LPARAM) -> (i32, i32) {
    (
        (lp.0 & 0xFFFF) as i16 as i32,
        ((lp.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}

/// Asks for a WM_MOUSELEAVE when the cursor leaves `hwnd`.
fn track_leave(hwnd: HWND) {
    let mut t = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut t);
    }
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

/// A popup window of one of our classes, with its own Direct2D target.
fn popup(
    class: PCWSTR,
    title: PCWSTR,
    ex: WINDOW_EX_STYLE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    k: f32,
) -> Option<(HWND, gfx::Gfx)> {
    unsafe {
        let hinst = HINSTANCE::from(GetModuleHandleW(None).unwrap_or_default());
        let hwnd = CreateWindowExW(
            ex | WS_EX_NOREDIRECTIONBITMAP,
            class,
            title,
            WS_POPUP | WS_SYSMENU,
            x,
            y,
            w,
            h,
            None,
            None,
            hinst,
            None,
        )
        .ok()?;
        let Ok(mut g) = gfx::Gfx::new(hwnd, None) else {
            let _ = DestroyWindow(hwnd);
            return None;
        };
        let _ = g.resize(w as u32, h as u32, k);
        Some((hwnd, g))
    }
}

struct PanelWin {
    hwnd: HWND,
    g: gfx::Gfx,
    st: panel::State,
    hits: Vec<panel::Hit>,
    dragging: Option<Action>, // the volume slider that has the mouse
    opened: Instant,
}

struct PrefsWin {
    hwnd: HWND,
    g: gfx::Gfx,
    st: prefs::State,
    hits: Vec<prefs::Hit>,
    opened: Instant,
}

/// What `apply` last pushed out to Windows and the workers, so it only redoes what changed.
#[derive(Default)]
struct Applied {
    hide_taskbar: Option<bool>,
    tray: Option<bool>,
    pins: Option<Vec<String>>,
    feeds: Option<FeedConfig>,
}

/// A monitor a bar docks on.
#[derive(Clone, Copy, PartialEq)]
struct Screen {
    mon: HMONITOR,
    rect: RECT,
}

/// The monitors to dock on: the primary one first, then with `every` the rest, left to right.
fn screens(every: bool) -> Vec<Screen> {
    unsafe extern "system" fn push(mon: HMONITOR, _: HDC, _: *mut RECT, l: LPARAM) -> BOOL {
        (*(l.0 as *mut Vec<HMONITOR>)).push(mon);
        TRUE
    }
    let mut mons: Vec<HMONITOR> = vec![];
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut mons as *mut _ as isize));
    }
    let mut v: Vec<(bool, Screen)> = mons
        .into_iter()
        .filter_map(|mon| {
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            unsafe { GetMonitorInfoW(mon, &mut mi) }.as_bool().then(|| {
                let primary = mi.dwFlags & MONITORINFOF_PRIMARY != 0;
                (
                    primary,
                    Screen {
                        mon,
                        rect: mi.rcMonitor,
                    },
                )
            })
        })
        .collect();
    v.sort_by_key(|(primary, s)| (!primary, s.rect.left, s.rect.top));
    if v.is_empty() {
        let mon = unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) };
        v.push((
            true,
            Screen {
                mon,
                rect: RECT::default(),
            },
        ));
    }
    if !every {
        v.truncate(1);
    }
    v.into_iter().map(|(_, s)| s).collect()
}

fn bar_window() -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE,
            w!("serenebar"),
            w!("serenebar"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            HINSTANCE::from(GetModuleHandleW(None)?),
            None,
        )
    }
}

/// One dock window on one monitor. The first bar, on the primary monitor, lives as long as the
/// app: the timers, the workers' notifications and the tray host all go to its window.
struct Bar {
    hwnd: HWND,
    g: gfx::Gfx,
    screen: Screen,
    hits: Vec<ui::Hit<Action>>,
    hover: Option<Action>,
    drag: Option<i32>, // cursor y minus window y, set while the dock is being dragged
    app_press: Option<(usize, i32)>, // app icon pressed: index, cursor x
    app_drag: Option<usize>, // app icon being dragged along the dock
    slide: Vec<f32>,   // per app: DIPs left to slide after a reorder
    band: RECT,        // the strip SHAppBarMessage reserved for us
    win: RECT,
    bar: RECT,  // the bar itself on screen. The window adds magnification room.
    bar_x: f32, // bar offset inside the window, DIPs
    bar_y: f32,
    region: RECT, // the part of the window that takes clicks, window pixels
    dpi: u32,
    inside: bool, // the cursor is over the window and a WM_MOUSELEAVE is on its way
    mag: Vec<f32>,
    mag_target: Vec<f32>,
    base_left: f32,                    // screen x where unmagnified content starts
    app_tip: Option<(String, String)>, // key and shell name of the last app hovered
}

impl Drop for Bar {
    fn drop(&mut self) {
        appbar(self.hwnd, ABM_REMOVE, 0, RECT::default());
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl Bar {
    fn new(hwnd: HWND, screen: Screen) -> Result<Self> {
        let g = gfx::Gfx::new(hwnd, Some(screen.mon))?;
        appbar(hwnd, ABM_NEW, 0, RECT::default());
        Ok(Bar {
            hwnd,
            g,
            screen,
            hits: vec![],
            hover: None,
            drag: None,
            app_press: None,
            app_drag: None,
            slide: vec![],
            band: RECT::default(),
            win: RECT::default(),
            bar: RECT::default(),
            bar_x: 0.0,
            bar_y: 0.0,
            region: RECT::default(),
            dpi: 96,
            inside: false,
            mag: vec![],
            mag_target: vec![],
            base_left: 0.0,
            app_tip: None,
        })
    }

    fn k(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    fn monitor(&self) -> RECT {
        self.screen.rect
    }

    /// Moves the bar to another monitor. The glass captures one monitor, so it starts over.
    fn rebind(&mut self, screen: Screen) {
        if screen == self.screen {
            return;
        }
        if let Ok(g) = gfx::Gfx::new(self.hwnd, Some(screen.mon)) {
            self.g = g;
        }
        self.screen = screen;
        self.win = RECT::default();
        self.region = RECT::default();
    }

    fn dock(&mut self, s: &Settings) {
        let (mut dx, mut dy) = (96, 96);
        unsafe {
            let _ = GetDpiForMonitor(self.screen.mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        }
        self.dpi = dx;
        let (bar_h, gap, _) = self.heights(s);
        let band_h = bar_h + 2 * gap;
        let edge = if s.top { ABE_TOP } else { ABE_BOTTOM };
        let mut rc = appbar(self.hwnd, ABM_QUERYPOS, edge, self.monitor());
        if s.top {
            rc.bottom = rc.top + band_h
        } else {
            rc.top = rc.bottom - band_h
        }
        self.band = appbar(self.hwnd, ABM_SETPOS, edge, rc);
    }

    fn load_icons(&self, d: &ui::Data) {
        let art_id = d.media.as_ref().map_or(0, |m| m.art_id);
        self.g.image("art", art_id, |g| {
            d.art.as_ref().and_then(|b| g.bitmap_from_bytes(b))
        });
        let px = ui::icon_px(self.k());
        for a in &d.apps {
            self.g.image(a.key(), px as u64, |g| {
                apps::app_image(&a.icon, px).and_then(|h| g.bitmap_from_hbitmap(h))
            });
        }
    }

    fn view<'a>(&'a self, s: &'a Settings, started: Instant, mag: &'a [f32]) -> ui::View<'a> {
        ui::View {
            s,
            th: ui::theme(s.style),
            m: ui::metrics(s.size),
            accent: ui::accent(s),
            hover: self.hover,
            mag,
            intro: started.elapsed().as_secs_f32(),
            slide: &self.slide,
            lifted: self.app_drag,
            bar_x: self.bar_x,
            bar_y: self.bar_y,
            tip: None,
        }
    }

    fn full_width(&self, s: &Settings) -> Option<f32> {
        let m = self.monitor();
        s.full.then(|| (m.right - m.left) as f32 / self.k())
    }

    /// The name to show over the hovered control. None while anything is being dragged.
    fn tip(&self, d: &ui::Data) -> Option<String> {
        if self.drag.is_some() || self.app_drag.is_some() {
            return None;
        }
        match self.hover? {
            Action::App(i) => {
                let (key, name) = self.app_tip.as_ref()?;
                (d.apps.get(i)?.key() == key).then(|| name.clone())
            }
            act => ui::tip(act, d),
        }
    }

    /// Looks up the shell name of the app under the cursor, once per app.
    fn name_hovered_app(&mut self, d: &ui::Data) {
        let Some(Action::App(i)) = self.hover else {
            return;
        };
        let Some(item) = d.apps.get(i) else { return };
        if self.app_tip.as_ref().is_none_or(|t| t.0 != item.key()) {
            self.app_tip = Some((item.key().to_string(), apps::app_name(item)));
        }
    }

    fn magnified(&self) -> bool {
        self.mag.iter().chain(&self.mag_target).any(|&m| m > 1.001)
    }

    /// The bar's height, its gap to the screen edge and the magnification room beside it, in pixels.
    fn heights(&self, s: &Settings) -> (i32, i32, i32) {
        let (m, k) = (ui::metrics(s.size), self.k());
        let gap = if s.full {
            0
        } else {
            (m.edge * k).round() as i32
        };
        (
            (m.h * k).ceil() as i32,
            gap,
            ((ui::head(&m) + ui::TIP_ROOM * m.k) * k).ceil() as i32,
        )
    }

    fn draw(&mut self, s: &Settings, d: &ui::Data, started: Instant) {
        let m = ui::metrics(s.size);
        let k = self.k();
        let monitor = self.monitor();
        let mon_w = monitor.right - monitor.left;
        let mag = std::mem::take(&mut self.mag);
        let content = ui::content_width(d, &self.view(s, started, &mag));
        let base = ui::content_width(d, &self.view(s, started, &[]));
        self.base_left = if s.full {
            monitor.left as f32
        } else {
            monitor.left as f32 + (mon_w as f32 - base * k) / 2.0
        };

        let bar_w = if s.full {
            mon_w
        } else {
            ((content * k).ceil() as i32).min(mon_w)
        };
        let (bar_h, gap, head) = self.heights(s);
        let r = |a: [i32; 4]| RECT {
            left: a[0],
            top: a[1],
            right: a[2],
            bottom: a[3],
        };
        let a = |r: RECT| [r.left, r.top, r.right, r.bottom];
        let (win, bar) =
            dock::dock_geometry(a(monitor), a(self.band), s.top, bar_w, bar_h, gap, head);
        let (win, mut bar) = (r(win), r(bar));
        if self.drag.is_none() {
            if win != self.win {
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        HWND_TOPMOST,
                        win.left,
                        win.top,
                        win.right - win.left,
                        win.bottom - win.top,
                        SWP_NOACTIVATE,
                    );
                }
                self.win = win;
            }
        } else {
            // the window follows the cursor, the bar keeps its place inside it
            let dy = bar.top - win.top;
            unsafe {
                let _ = GetWindowRect(self.hwnd, &mut self.win);
            }
            bar = RECT {
                top: self.win.top + dy,
                bottom: self.win.top + dy + bar_h,
                ..bar
            };
        }
        self.bar = bar;
        (self.bar_x, self.bar_y) = (
            (bar.left - self.win.left) as f32 / k,
            (bar.top - self.win.top) as f32 / k,
        );
        let _ = self.g.resize(
            (self.win.right - self.win.left) as u32,
            (self.win.bottom - self.win.top) as u32,
            k,
        );

        // Grow the clickable region before drawing, shrink it after, so it never cuts into a frame on screen.
        let tip = self.tip(d);
        let open = (self.magnified() || tip.is_some()) && self.drag.is_none();
        if open {
            self.set_region(true);
        }
        let glass = crate::glass::params(s.style)
            .is_some_and(|p| self.g.prepare_glass(self.bar, m.bar_radius(s.full), p));
        let view = ui::View {
            tip,
            ..self.view(s, started, &mag)
        };
        self.g.begin();
        ui::draw_bar(&self.g, &view, bar_w as f32 / k, glass);
        let hits = ui::draw_tiles(&self.g, &view, d, self.full_width(s));
        self.g.end();
        self.hits = hits;
        if !open {
            self.set_region(false);
        }
        self.mag = mag;
    }

    /// Clicks outside the region fall through to whatever is behind. `open` adds the magnification room.
    fn set_region(&mut self, open: bool) {
        let (l, r) = (
            self.bar.left - self.win.left,
            self.bar.right - self.win.left,
        );
        let (t, b) = if open {
            (0, self.win.bottom - self.win.top)
        } else {
            (self.bar.top - self.win.top, self.bar.bottom - self.win.top)
        };
        let want = RECT {
            left: l,
            top: t,
            right: r,
            bottom: b,
        };
        if want == self.region {
            return;
        }
        self.region = want;
        unsafe {
            let rgn = CreateRectRgn(l, t, r, b);
            SetWindowRgn(self.hwnd, rgn, false); // the system owns rgn from here on
        }
    }

    /// What's under a point given in window pixels.
    fn hit_at(&self, (x, y): (i32, i32)) -> Option<(Action, f32)> {
        let k = self.k();
        ui::hit_test(
            &self.hits,
            x as f32 / k - self.bar_x,
            y as f32 / k - self.bar_y,
        )
    }

    /// Ranges of the icon tiles, unmagnified icon centres, and the cursor's x, all in content DIPs.
    fn icons_and_cursor(
        &self,
        s: &Settings,
        d: &ui::Data,
        started: Instant,
    ) -> (Vec<(f32, f32)>, Vec<f32>, f32) {
        let (ranges, centres) = ui::icon_layout(d, &self.view(s, started, &[]), self.full_width(s));
        (
            ranges,
            centres,
            (cursor().x as f32 - self.base_left) / self.k(),
        )
    }

    /// Moves the dragged app icon to the slot nearest the cursor. The order is saved on release.
    fn drag_app(&mut self, s: &Settings, d: &mut ui::Data, started: Instant) {
        let Some(from) = self.app_drag else { return };
        let (_, centres, cx) = self.icons_and_cursor(s, d, started);
        let step = ui::icon_step(&ui::metrics(s.size));
        let to = centres[1..]
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - cx).abs().total_cmp(&(b.1 - cx).abs()))
            .map(|(j, _)| j);
        if let Some(to) = to.filter(|&to| to != from) {
            let item = d.apps.remove(from);
            d.apps.insert(to, item);
            let held = self.slide.remove(from);
            self.slide.insert(to, held);
            // the icons it passed slide over by one place
            let (lo, hi, dir) = if to > from {
                (from, to, 1.0)
            } else {
                (to + 1, from + 1, -1.0)
            };
            for s in &mut self.slide[lo..hi] {
                *s += dir * step;
            }
            self.app_drag = Some(to);
        }
        // the held icon stays under the cursor
        let at = self.app_drag.unwrap_or(from);
        if let Some(c) = centres.get(at + 1) {
            self.slide[at] = cx - c;
        }
    }

    /// Targets follow the cursor while it is over a dock icon tile and fall back to 1 otherwise.
    fn update_targets(&mut self, s: &Settings, d: &ui::Data, started: Instant) {
        let (ranges, centres, cx) = self.icons_and_cursor(s, d, started);
        let m = ui::metrics(s.size);
        let (y, head) = (cursor().y as f32, ui::head(&m) * self.k());
        let in_band = y >= self.bar.top as f32 - if s.top { 0.0 } else { head }
            && y <= self.bar.bottom as f32 + if s.top { head } else { 0.0 };
        let over = self.inside
            && self.drag.is_none()
            && in_band
            && ranges.iter().any(|&(x0, x1)| cx >= x0 && cx <= x1);
        let n = d.apps.len() + 1;
        self.mag_target = if over {
            centres
                .iter()
                .map(|c| {
                    if c.is_nan() {
                        1.0
                    } else {
                        dock::mag_scale((cx - c).abs(), ui::icon_step(&m))
                    }
                })
                .collect()
        } else {
            vec![1.0; n]
        };
        self.mag_target.resize(n, 1.0);
    }

    /// One animation frame of the slides and the magnification. Returns whether anything moved.
    fn anim_step(&mut self, s: &Settings, d: &ui::Data, started: Instant) -> bool {
        let tiles = ui::layout(d, &self.view(s, started, &[]), self.full_width(s)).len();
        let mut moving = started.elapsed().as_secs_f32() < ui::intro_end(tiles);
        for (i, sl) in self.slide.iter_mut().enumerate() {
            if self.app_drag == Some(i) {
                continue;
            }
            if sl.abs() < 0.3 {
                *sl = 0.0
            } else {
                *sl *= 0.7;
                moving = true;
            }
        }
        if self.inside {
            self.update_targets(s, d, started);
        }
        for (m, t) in self.mag.iter_mut().zip(&self.mag_target) {
            let dm = t - *m;
            if dm.abs() < 0.003 {
                *m = *t
            } else {
                *m += dm * 0.3;
                moving = true;
            }
        }
        self.draw(s, d, started);
        moving
    }
}

struct App {
    s: Settings,
    applied: Applied,
    capturable: bool, // "Show in screenshots" is on, so the glass is frozen
    d: ui::Data,
    pinned: Vec<Pinned>,
    app_order: Vec<String>,   // first-seen order of unpinned apps
    start_left: Option<bool>, // Start menu alignment last applied
    cpu_prev: (u64, u64),
    net_prev: Option<(Instant, u64, u64)>, // when the network counters were last read
    art_id: u64,
    tx: Sender<Cmd>,
    feeds: Sender<feeds::Msg>,
    explorer: Option<HWND>, // Explorer's taskbar when we last took it over
    started: Instant,
    animating: bool,
    bars: Vec<Bar>, // never empty; bars[0] is on the primary monitor
    at: usize,      // the bar the user last pointed at. Panels and Settings open from it.
    panel: Option<PanelWin>,
    prefs: Option<PrefsWin>,
    installer: Option<std::path::PathBuf>, // a newer release's setup, downloaded and checked
    installing: bool,                      // it's been started; setup closes this app
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// SHAppBarMessage and popup menus call back into the wndproc while App is borrowed.
/// Those nested calls fall through to DefWindowProc.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Like `with_app`, for input to one bar: `f` gets that bar's index, and it becomes `at`.
fn with_bar<R>(hwnd: HWND, f: impl FnOnce(&mut App, usize) -> R) -> Option<R> {
    with_app(|a| {
        let i = a.bars.iter().position(|b| b.hwnd == hwnd)?;
        a.at = i;
        Some(f(a, i))
    })
    .flatten()
}

fn appbar(hwnd: HWND, msg: u32, edge: u32, rc: RECT) -> RECT {
    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: WM_APPBAR,
        uEdge: edge,
        rc,
        ..Default::default()
    };
    unsafe { SHAppBarMessage(msg, &mut abd) };
    abd.rc
}

fn feed_config(s: &Settings) -> FeedConfig {
    FeedConfig {
        city: s.city.clone(),
        lat: s.lat,
        lon: s.lon,
        fahrenheit: s.fahrenheit,
        calendar: s.calendar.clone(),
        google_id: s.google_id.clone(),
        google_secret: s.google_secret.clone(),
        deepseek_key: s.deepseek_key.clone(),
        antigravity: s.shows(Widget::Antigravity),
    }
}

impl App {
    /// The window timers, worker notifications and the tray host talk to.
    fn main(&self) -> HWND {
        self.bars[0].hwnd
    }

    /// The bar the user is working with.
    fn here(&self) -> &Bar {
        &self.bars[self.at]
    }

    /// Every settings change goes through here.
    fn change(&mut self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.s);
        self.s.save();
        self.apply();
    }

    /// Brings Windows, the workers and every window in line with the settings.
    fn apply(&mut self) {
        if self.applied.hide_taskbar != Some(self.s.hide_taskbar) {
            taskbar::taskbar_take(self.s.hide_taskbar);
            self.applied.hide_taskbar = Some(self.s.hide_taskbar);
        }
        let tray = self.s.shows(Widget::Tray); // the tray host only exists while its tile is on
        if self.applied.tray != Some(tray) {
            if tray {
                tray::start(self.main())
            } else {
                tray::stop()
            }
            self.applied.tray = Some(tray);
        }
        if self.applied.pins.as_ref() != Some(&self.s.pins) {
            self.pinned = apps::pinned(&self.s.pins);
            self.applied.pins = Some(self.s.pins.clone());
        }
        let cfg = feed_config(&self.s);
        if self.applied.feeds.as_ref() != Some(&cfg) {
            let _ = self.feeds.send(feeds::Msg::Config(cfg.clone()));
            self.applied.feeds = Some(cfg);
        }
        self.refresh();
        self.dock();
        self.draw_panel();
        self.draw_prefs();
    }

    fn glass_live(&self) -> bool {
        crate::glass::params(self.s.style).is_some() && !self.capturable
    }

    /// Puts a bar on each monitor the settings ask for, reserves its strip and draws it.
    fn dock(&mut self) {
        let want = screens(self.s.every_monitor);
        self.bars.truncate(want.len());
        for (i, &screen) in want.iter().enumerate() {
            match self.bars.get_mut(i) {
                Some(b) => b.rebind(screen),
                None => {
                    let Ok(b) = bar_window().and_then(|h| Bar::new(h, screen)) else {
                        break;
                    };
                    unsafe {
                        let _ = ShowWindow(b.hwnd, SW_SHOWNOACTIVATE);
                    }
                    self.bars.push(b);
                }
            }
        }
        self.at = self.at.min(self.bars.len() - 1);
        // the glass samples the screen, so while it's live the bar stays out of every capture
        let mode = if self.glass_live() {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        }; // needs Windows 10 2004+
        for b in &mut self.bars {
            unsafe {
                let _ = SetWindowDisplayAffinity(b.hwnd, mode);
            }
            b.dock(&self.s);
            b.load_icons(&self.d);
        }
        self.draw_all();
    }

    /// Reads what every source knows now, and loads icons the dock hasn't seen yet.
    fn refresh(&mut self) {
        self.d.cpu = system::cpu(&mut self.cpu_prev);
        self.d.mem = system::mem();
        self.d.battery = system::battery();
        self.read_network();
        if self.bars.iter().all(|b| b.app_drag.is_none()) {
            self.d.apps = apps::apps(
                &self.pinned,
                self.main(),
                &mut self.app_order,
                &self.s.app_order,
            );
        }
        self.d.tray = tray::icons();
        self.d.volume = controls::volume();
        let mixer = self
            .panel
            .as_ref()
            .is_some_and(|p| p.st.kind == Panel::Controls);
        self.d.app_volumes = if mixer {
            controls::app_volumes()
        } else {
            vec![]
        };
        self.d.mic = controls::mic_muted();
        self.d.light = system::light_theme();
        self.d.lang = system::input_language();
        self.d.now = crate::dates::unix_now();
        self.d.time = system::local_time();
        let f = feeds::snapshot();
        (self.d.weather, self.d.events, self.d.claude, self.d.codex) =
            (f.weather, f.events, f.claude, f.codex);
        (self.d.antigravity, self.d.deepseek, self.d.cursor) =
            (f.antigravity, f.deepseek, f.cursor);
        (self.d.gmail, self.d.google) = (f.gmail, google::connected());
        (self.d.update, self.d.update_note, self.installer) =
            (f.update, f.update_note, f.installer);
        self.d.weather_w = self.d.weather.as_ref().map_or(0.0, |wx| {
            let g = &self.bars[0].g;
            let label = weather::wmo(wx.code, wx.day).0;
            // at the sizes the tile draws them, then back to scale 1
            let k = ui::metrics(self.s.size).k;
            let text = g.measure(label, ui::font(k, 11.0), 600);
            let city = g.measure(&wx.city, ui::font(k, 10.0), 500);
            let temp = g.measure(&format!("{:.0}°", wx.temp), ui::font(k, 21.0), 300);
            (text.max(city) + temp) / k
        });
        let c = controls::snapshot();
        let id = c.media.as_ref().map_or(0, |m| m.art_id);
        if id != self.art_id {
            self.art_id = id;
            self.d.art_tint = c.art.as_ref().and_then(|b| self.bars[0].g.average_color(b));
            self.d.art = c.art.clone();
        }
        (self.d.media, self.d.wifi, self.d.bluetooth, self.d.vpn) =
            (c.media, c.wifi, c.bluetooth, c.vpn);
        (self.d.airplane, self.d.hotspot) = (c.airplane, c.hotspot);
        let n = self.d.apps.len() + 1;
        for b in &mut self.bars {
            b.load_icons(&self.d);
            b.slide.resize(self.d.apps.len(), 0.0);
            b.mag.resize(n, 1.0);
            b.mag_target.resize(n, 1.0);
        }
    }

    /// Speeds from the counters' change since the last reading, at most twice a second.
    fn read_network(&mut self) {
        let Some((rx, tx)) = crate::net::counters() else {
            return;
        };
        let now = Instant::now();
        match self.net_prev {
            Some((t, _, _)) if now.duration_since(t).as_secs_f64() < 0.5 => {}
            Some((t, prx, ptx)) => {
                let dt = now.duration_since(t).as_secs_f64();
                self.d.net = Some((crate::net::rate(prx, rx, dt), crate::net::rate(ptx, tx, dt)));
                self.net_prev = Some((now, rx, tx));
            }
            None => self.net_prev = Some((now, rx, tx)),
        }
    }

    fn update(&mut self) {
        self.refresh();
        if self.s.auto_update && self.installer.is_some() && !self.installing && update::away() {
            self.install();
        }
        self.draw_all();
        self.draw_panel();
        self.draw_prefs();
    }

    fn install(&mut self) {
        if let Some(p) = self.installer.as_ref().filter(|_| !self.installing) {
            self.installing = update::install(p, system::startup_enabled());
        }
    }

    fn draw(&mut self, i: usize) {
        self.bars[i].draw(&self.s, &self.d, self.started);
        if i == 0 {
            self.align_start();
        }
    }

    fn draw_all(&mut self) {
        for i in 0..self.bars.len() {
            self.draw(i);
        }
    }

    /// Start opens centred or at the left on Windows 11; pick whichever is nearer the primary
    /// bar's Start tile.
    fn align_start(&mut self) {
        let b = &self.bars[0];
        let Some((x, w)) = ui::hit_span(&b.hits, Action::Start) else {
            return;
        };
        let centre = b.bar.left as f32 + (x + w / 2.0) * b.k(); // hits are relative to the bar
        let m = b.monitor();
        let left = centre < m.left as f32 + (m.right - m.left) as f32 / 3.0;
        if self.start_left != Some(left) {
            self.start_left = Some(left);
            taskbar::align_start(left);
        }
    }

    fn update_targets(&mut self, i: usize) {
        self.bars[i].update_targets(&self.s, &self.d, self.started);
        self.animate();
    }

    fn drag_app(&mut self, i: usize) {
        self.bars[i].drag_app(&self.s, &mut self.d, self.started);
        self.animate();
        self.draw_all(); // the other bars show the new order too
    }

    fn animate(&mut self) {
        if !self.animating {
            self.animating = true;
            unsafe {
                SetTimer(self.main(), TIMER_ANIM, 15, None);
            }
        }
    }

    fn anim_step(&mut self) {
        let mut moving = false;
        for b in &mut self.bars {
            moving |= b.anim_step(&self.s, &self.d, self.started);
        }
        self.align_start();
        if self
            .d
            .theme_swap
            .is_some_and(|t| t.elapsed().as_secs_f32() < ui::THEME_SWAP + 0.05)
        {
            moving = true;
        }
        if self.panel.as_ref().is_some_and(|p| still_opening(p.opened)) {
            moving = true;
            self.draw_panel();
        }
        if self.prefs.as_ref().is_some_and(|p| still_opening(p.opened)) {
            moving = true;
            self.draw_prefs();
        }
        if !moving {
            self.animating = false;
            unsafe {
                let _ = KillTimer(self.main(), TIMER_ANIM);
            }
        }
    }

    /// Everything the user can do from the bar, its panels and its menus. `frac` is how far
    /// across its box the click was, for sliders.
    fn act(&mut self, a: Action, frac: f32) {
        match a {
            Action::Start => system::press_keys(&[VK_LWIN]),
            Action::Language => system::press_keys(&[VK_LWIN, VK_SPACE]),
            Action::ShowDesktop => system::press_keys(&[VK_LWIN, VK_D]),
            Action::Notifications => system::press_keys(&[VK_LWIN, VK_N]),
            Action::QuickSettings => system::press_keys(&[VK_LWIN, VK_A]), // Action Center on Windows 10
            Action::SystemSettings => system::open("ms-settings:"),
            Action::SettingsFolder => {
                if let Some(dir) = Settings::path().parent() {
                    system::open(&dir.to_string_lossy())
                }
            }
            Action::TaskManager => system::open("taskmgr.exe"),
            Action::DeepSeek => system::open("https://platform.deepseek.com/usage"),
            Action::Gmail => system::open("https://mail.google.com/"),
            // the release page when its installer couldn't be fetched or checked
            Action::Update => match (&self.installer, &self.d.update) {
                (Some(_), _) => self.install(),
                (None, Some(r)) => system::open(&r.url),
                (None, None) => {}
            },
            Action::App(i) => {
                if let Some(item) = self.d.apps.get(i) {
                    apps::activate(item)
                }
            }
            Action::Prev => {
                let _ = self.tx.send(Cmd::Prev);
            }
            Action::Toggle => {
                let _ = self.tx.send(Cmd::Toggle);
            }
            Action::Next => {
                let _ = self.tx.send(Cmd::Next);
            }
            Action::Wifi => {
                let _ = self.tx.send(Cmd::Wifi);
            }
            Action::Bluetooth => {
                let _ = self.tx.send(Cmd::Bluetooth);
            }
            Action::Vpn => {
                let _ = self.tx.send(Cmd::Vpn);
            }
            Action::Airplane => {
                let _ = self.tx.send(Cmd::Airplane);
            }
            Action::Hotspot => {
                let _ = self.tx.send(Cmd::Hotspot);
            }
            Action::Mute => controls::toggle_mute(),
            Action::Mic => controls::toggle_mic(),
            Action::AppMute(i) => {
                if let Some(a) = self.d.app_volumes.get(i) {
                    controls::toggle_app_mute(&a.name);
                }
            }
            Action::AppVolume(_) => {} // dragged in the panel
            Action::Theme => {
                system::toggle_theme();
                self.d.theme_swap = Some(Instant::now());
                self.animate();
            }
            Action::Volume => {
                self.set_volume(frac);
                return;
            }
            Action::Settings => {
                self.open_prefs();
                return;
            }
            Action::Open(kind) => {
                self.toggle_panel(kind);
                return;
            }
        }
        self.update();
    }

    fn toggle_panel(&mut self, kind: Panel) {
        let same = self.panel.as_ref().is_some_and(|p| p.st.kind == kind);
        self.close_panel();
        if same {
            return;
        }
        let (b, k) = (self.here(), self.here().k());
        let (bar, monitor) = (b.bar, b.monitor());
        // anchored over its tile; panels without one (About) open over the middle of the bar
        let (hx, hw) = ui::hit_span(&b.hits, Action::Open(kind))
            .unwrap_or((0.0, (bar.right - bar.left) as f32 / k));
        if kind == Panel::Controls {
            self.d.app_volumes = controls::app_volumes();
        }
        let (pw, ph) = panel::size(kind, &self.d);
        let (w, h) = ((pw * k).ceil() as i32, (ph * k).ceil() as i32);
        let gap = (10.0 * k) as i32;
        let centre = bar.left + ((hx + hw / 2.0) * k) as i32;
        let x = (centre - w / 2).clamp(monitor.left + gap, monitor.right - w - gap);
        let y = if self.s.top {
            bar.bottom + gap
        } else {
            bar.top - gap - h
        };
        let Some((hwnd, g)) = popup(
            w!("serenebar-panel"),
            w!(""),
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            x,
            y,
            w,
            h,
            k,
        ) else {
            return;
        };
        let mut st = panel::State::new(kind, self.d.time);
        st.app_rows = self.d.app_volumes.len().min(panel::APP_ROWS);
        self.panel = Some(PanelWin {
            hwnd,
            g,
            st,
            hits: vec![],
            dragging: None,
            opened: Instant::now(),
        });
        self.show(hwnd);
    }

    fn show(&mut self, hwnd: HWND) {
        self.draw_panel();
        self.draw_prefs();
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        self.animate();
    }

    fn close_panel(&mut self) {
        if let Some(p) = self.panel.take() {
            unsafe {
                let _ = DestroyWindow(p.hwnd);
            }
        }
    }

    fn draw_panel(&mut self) {
        let Some(p) = &mut self.panel else { return };
        if p.st.kind == Panel::Tray {
            for i in &self.d.tray {
                p.g.image(&i.key(), i.stamp, |g| g.bitmap_from_hicon(i.icon));
            }
        }
        let (w, h) = (p.g.w as f32 / p.g.k, p.g.h as f32 / p.g.k);
        let (o, sc, dy) = opening(p.opened);
        p.g.begin();
        // panels rise out of the dock, so they grow from the edge nearest it
        p.g.push_fade(
            o,
            sc,
            w / 2.0,
            if self.s.top { 0.0 } else { h },
            if self.s.top { -dy } else { dy },
        );
        p.hits = panel::draw(&p.g, &p.st, &self.d, &self.s, w, h);
        p.g.pop_fade();
        p.g.end();
    }

    fn panel_hit(&self, lp: LPARAM) -> Option<(panel::Act, f32)> {
        let p = self.panel.as_ref()?;
        let (x, y) = xy(lp);
        ui::hit_test(&p.hits, x as f32 / p.g.k, y as f32 / p.g.k)
    }

    /// Sets the volume from where the cursor is along the slider being dragged, even outside it.
    fn panel_volume(&mut self, lp: LPARAM) {
        let Some(p) = &self.panel else { return };
        let Some(act) = p.dragging else { return };
        let Some(level) = panel::volume_at(&p.hits, act, xy(lp).0 as f32 / p.g.k) else {
            return;
        };
        match act {
            Action::AppVolume(i) => {
                if let Some(a) = self.d.app_volumes.get(i) {
                    controls::set_app_volume(&a.name, level);
                }
                self.d.app_volumes = controls::app_volumes();
                self.draw_panel();
            }
            _ => self.set_volume(level),
        }
    }

    fn set_volume(&mut self, level: f32) {
        controls::set_volume(level);
        self.d.volume = controls::volume();
        self.draw_all();
        self.draw_panel();
    }

    fn panel_press(&mut self, lp: LPARAM) {
        if let Some((panel::Act::Do(a @ (Action::Volume | Action::AppVolume(_))), _)) =
            self.panel_hit(lp)
        {
            if let Some(p) = &mut self.panel {
                p.dragging = Some(a);
                unsafe {
                    SetCapture(p.hwnd);
                }
            }
            self.panel_volume(lp);
        }
    }

    fn panel_move(&mut self, lp: LPARAM) {
        if self.panel.as_ref().is_some_and(|p| p.dragging.is_some()) {
            self.panel_volume(lp);
            return;
        }
        let over = self.panel_hit(lp).map(|h| h.0);
        let hover = match over {
            Some(panel::Act::TrayIcon(i)) => Some(i),
            _ => None,
        };
        if let Some(p) = self
            .panel
            .as_mut()
            .filter(|p| (p.st.hover, p.st.over) != (hover, over))
        {
            track_leave(p.hwnd);
            (p.st.hover, p.st.over) = (hover, over);
            self.draw_panel();
        }
    }

    /// Clicks a hidden icon for its app. The panel closes first, like the taskbar's does.
    fn tray_click(&mut self, i: usize, right: bool) {
        let Some(icon) = self.d.tray.get(i).cloned() else {
            return;
        };
        self.close_panel();
        tray::click(&icon, right, cursor());
    }

    fn panel_click(&mut self, lp: LPARAM) {
        if let Some(p) = self.panel.as_mut().filter(|p| p.dragging.is_some()) {
            p.dragging = None;
            unsafe {
                let _ = ReleaseCapture();
            }
            return;
        }
        let Some((act, frac)) = self.panel_hit(lp) else {
            return;
        };
        let Some(p) = &mut self.panel else { return };
        if p.st.on(act, self.d.time) {
            self.draw_panel();
            return;
        }
        match act {
            panel::Act::Close => self.close_panel(),
            panel::Act::TrayIcon(i) => self.tray_click(i, false),
            panel::Act::Do(Action::Volume | Action::AppVolume(_)) => {} // the press set it
            // anything that opens another window takes the focus, which closes the panel
            panel::Act::Do(a) => self.act(a, frac),
            _ => {}
        }
    }

    fn open_prefs(&mut self) {
        if let Some(p) = &self.prefs {
            unsafe {
                let _ = SetForegroundWindow(p.hwnd);
            }
            return;
        }
        let (k, m) = (self.here().k(), self.here().monitor());
        let (w, h) = (
            (prefs::SIZE.0 * k).ceil() as i32,
            (prefs::SIZE.1 * k).ceil() as i32,
        );
        let x = m.left + (m.right - m.left - w) / 2;
        let y = m.top + (m.bottom - m.top - h) / 2;
        let Some((hwnd, g)) = popup(
            w!("serenebar-settings"),
            w!("serenebar settings"),
            WS_EX_APPWINDOW,
            x,
            y,
            w,
            h,
            k,
        ) else {
            return;
        };
        self.prefs = Some(PrefsWin {
            hwnd,
            g,
            st: prefs::State::new(&self.s),
            hits: vec![],
            opened: Instant::now(),
        });
        self.show(hwnd);
    }

    fn close_prefs(&mut self) {
        self.commit_field();
        if let Some(p) = self.prefs.take() {
            unsafe {
                let _ = DestroyWindow(p.hwnd);
            }
        }
    }

    fn draw_prefs(&mut self) {
        if self.prefs.is_none() {
            return;
        }
        let flags = prefs::Flags {
            startup: system::startup_enabled(),
            screenshots: self.capturable,
            google: google::connected(),
            google_status: google::status(),
            google_account: google::account(),
            update: self.d.update.as_ref().map(|r| r.version.clone()),
            update_note: self.d.update_note,
        };
        let Some(p) = &mut self.prefs else { return };
        let city = prefs::Field::City;
        p.st.places = if p.st.focus == Some(city) {
            weather::suggestions(p.st.get(city))
        } else {
            vec![]
        };
        let (w, h) = (p.g.w as f32 / p.g.k, p.g.h as f32 / p.g.k);
        let (o, sc, dy) = opening(p.opened);
        p.g.begin();
        p.g.push_fade(o, sc, w / 2.0, h / 2.0, dy);
        p.hits = prefs::draw(&p.g, &p.st, &self.s, flags, w, h);
        p.g.pop_fade();
        p.g.end();
    }

    /// Writes the text fields to the settings. Runs when a field loses focus.
    fn commit_field(&mut self) {
        let Some(p) = &mut self.prefs else { return };
        if p.st.focus.take().is_none() {
            return;
        }
        let mut new = self.s.clone();
        p.st.write_fields(&mut new);
        self.change(|s| *s = new);
    }

    fn prefs_hit(&self, lp: LPARAM) -> Option<prefs::Act> {
        let p = self.prefs.as_ref()?;
        let (x, y) = xy(lp);
        ui::hit_test(&p.hits, x as f32 / p.g.k, y as f32 / p.g.k).map(|h| h.0)
    }

    /// A press on a widget row's label or grip picks it up.
    fn prefs_press(&mut self, lp: LPARAM) {
        let Some(prefs::Act::Drag(row)) = self.prefs_hit(lp) else {
            return;
        };
        let Some(p) = &mut self.prefs else { return };
        p.st.press(row, xy(lp).1 as f32 / p.g.k);
        unsafe {
            SetCapture(p.hwnd);
        }
    }

    fn prefs_move(&mut self, lp: LPARAM) {
        let over = self.prefs_hit(lp);
        let Some(p) = &mut self.prefs else { return };
        let moved = p.st.drag_to(xy(lp).1 as f32 / p.g.k);
        let over = if p.st.dragging() { None } else { over };
        if moved || p.st.over != over {
            track_leave(p.hwnd);
            p.st.over = over;
            self.draw_prefs();
        }
    }

    /// The cursor left a panel or the settings window: nothing is under it any more.
    fn leave_popup(&mut self) {
        if let Some(p) = self.panel.as_mut().filter(|p| p.st.over.is_some()) {
            (p.st.hover, p.st.over) = (None, None);
            self.draw_panel();
        }
        if let Some(p) = self.prefs.as_mut().filter(|p| p.st.over.is_some()) {
            p.st.over = None;
            self.draw_prefs();
        }
    }

    fn prefs_click(&mut self, lp: LPARAM) {
        if let Some(p) = self.prefs.as_mut().filter(|p| p.st.dragging()) {
            unsafe {
                let _ = ReleaseCapture();
            }
            if p.st.drop() {
                let widgets = widgets_from_rows(&p.st.rows);
                self.change(|s| s.widgets = widgets);
                return;
            }
        }
        let act = self.prefs_hit(lp);
        // leaving a field, for anything but a click back into the same one, applies it
        // and picking a suggestion applies it with its exact spot
        let same_field = act
            == self
                .prefs
                .as_ref()
                .and_then(|p| p.st.focus.map(prefs::Act::Focus));
        if !same_field && !matches!(act, Some(prefs::Act::Pick(_))) {
            self.commit_field();
        }
        if let Some(act) = act {
            self.apply_pref(act);
        }
        self.draw_prefs();
    }

    fn apply_pref(&mut self, act: prefs::Act) {
        use prefs::Act;
        match act {
            Act::Close => self.close_prefs(),
            Act::Quit => {
                self.close_prefs();
                unsafe { PostQuitMessage(0) };
            }
            Act::Style(st) => self.change(|s| s.style = st),
            Act::Accent(i) => self.change(|s| s.accent = i),
            Act::Size(z) => self.change(|s| s.size = z),
            Act::Top(t) => self.change(|s| s.top = t),
            Act::Full(f) => self.change(|s| s.full = f),
            Act::EveryMonitor(e) => self.change(|s| s.every_monitor = e),
            Act::HideTaskbar(h) => self.change(|s| s.hide_taskbar = h),
            Act::Clock24(c) => self.change(|s| s.clock24 = c),
            Act::Fahrenheit(f) => self.change(|s| s.fahrenheit = f),
            Act::Startup => system::set_startup(!system::startup_enabled()),
            Act::Screenshots => {
                self.capturable = !self.capturable;
                self.apply();
            }
            Act::Control(c) => self.change(|s| {
                match s.controls.iter().position(|x| *x == c) {
                    Some(i) => {
                        s.controls.remove(i);
                    }
                    None => s.controls.push(c),
                }
                s.controls.sort_by_key(|c| *c as usize);
            }),
            Act::Remove(i) => {
                let Some(p) = &mut self.prefs else { return };
                p.st.remove(i);
                let widgets = widgets_from_rows(&p.st.rows);
                self.change(|s| s.widgets = widgets);
            }
            Act::Add(group) => {
                let Some(p) = &mut self.prefs else { return };
                let hidden: Vec<usize> =
                    (0..p.st.rows.len()).filter(|&i| !p.st.rows[i].on).collect();
                let cmd = popup_menu(p.hwnd, true, |m| unsafe {
                    for (n, &i) in hidden.iter().enumerate() {
                        let label = HSTRING::from(p.st.rows[i].widget.label());
                        let _ = AppendMenuW(m, MF_STRING, n + 1, &label);
                    }
                });
                let Some(&i) = (cmd as usize).checked_sub(1).and_then(|n| hidden.get(n)) else {
                    return;
                };
                p.st.add(i, group);
                let widgets = widgets_from_rows(&p.st.rows);
                self.change(|s| s.widgets = widgets);
            }
            Act::Focus(f) => {
                if let Some(p) = &mut self.prefs {
                    p.st.focus(f)
                }
            }
            Act::Pick(i) => {
                let Some(p) = &mut self.prefs else { return };
                let Some(place) = p.st.pick(i) else { return };
                let mut new = self.s.clone();
                p.st.write_fields(&mut new);
                (new.lat, new.lon) = (Some(place.lat), Some(place.lon));
                self.change(|s| *s = new);
            }
            Act::Tab(t) => {
                if let Some(p) = &mut self.prefs {
                    p.st.tab = t
                }
            }
            Act::Drag(_) => {}
            Act::Variant(i) => {
                let Some(p) = &self.prefs else { return };
                let Some(w) = p.st.rows.get(i).map(|r| r.widget) else {
                    return;
                };
                let (all, now) = (w.variants(), self.s.variant(w));
                let cmd = popup_menu(p.hwnd, true, |m| unsafe {
                    for (n, x) in all.iter().enumerate() {
                        let flags = if *x == now { MF_CHECKED } else { MF_UNCHECKED };
                        let _ = AppendMenuW(m, flags, n + 1, &HSTRING::from(x.label()));
                    }
                });
                if let Some(&x) = (cmd as usize).checked_sub(1).and_then(|n| all.get(n)) {
                    self.change(|s| s.set_variant(w, x));
                }
            }
            Act::About => self.toggle_panel(Panel::About),
            Act::GoogleSignIn => google::sign_in(
                self.s.google_id.clone(),
                self.s.google_secret.clone(),
                self.after_google(),
            ),
            Act::GoogleSignOut => google::sign_out(self.after_google()),
            Act::Update => self.act(Action::Update, 0.0),
            Act::AutoUpdate => self.change(|s| s.auto_update = !s.auto_update),
            Act::CheckUpdate => {
                let _ = self.feeds.send(feeds::Msg::CheckUpdate);
            }
            Act::GoogleGuide => system::open(
                "https://github.com/gxjakkap/serenebar/blob/main/docs/google-calendar.md",
            ),
        }
    }

    /// Once Google sign-in or sign-out finishes: refetch the calendar and refresh Settings.
    fn after_google(&self) -> impl FnOnce() + Send + 'static {
        let (feeds, h) = (self.feeds.clone(), self.main().0 as isize);
        move || {
            let _ = feeds.send(feeds::Msg::Refetch);
            system::notify(h);
        }
    }

    fn prefs_char(&mut self, ch: u32) {
        let Some(p) = &mut self.prefs else { return };
        let hwnd = p.hwnd;
        if p.st.type_char(ch, || clipboard_text(hwnd)) {
            self.commit_field();
            return;
        }
        self.city_typed();
        self.draw_prefs();
    }

    fn prefs_key(&mut self, vk: u16) {
        let Some(p) = &mut self.prefs else { return };
        if vk == VK_ESCAPE.0 {
            if p.st.focus.is_some() {
                self.commit_field();
            } else {
                self.close_prefs();
            }
            return;
        }
        if p.st.key(VIRTUAL_KEY(vk)) {
            self.city_typed();
            self.draw_prefs();
        }
    }

    /// Restarts the pause before the city suggestions are looked up.
    fn city_typed(&self) {
        if self
            .prefs
            .as_ref()
            .is_some_and(|p| p.st.focus == Some(prefs::Field::City))
        {
            unsafe {
                SetTimer(self.main(), TIMER_SUGGEST, 350, None);
            }
        }
    }

    fn suggest_city(&self) {
        unsafe {
            let _ = KillTimer(self.main(), TIMER_SUGGEST);
        }
        let Some(p) = self
            .prefs
            .as_ref()
            .filter(|p| p.st.focus == Some(prefs::Field::City))
        else {
            return;
        };
        let query = p.st.get(prefs::Field::City).to_string();
        if query.chars().count() < 2 {
            return;
        }
        let h = self.main().0 as isize;
        weather::suggest(query, move || system::notify(h));
    }

    fn app_menu(&mut self, i: usize) {
        let Some(item) = self.d.apps.get(i) else {
            return;
        };
        let (name, pin, running) = (apps::app_name(item), item.pin, !item.windows.is_empty());
        let cmd = popup_menu(self.here().hwnd, self.s.top, |m| unsafe {
            let _ = AppendMenuW(
                m,
                MF_STRING,
                2,
                &HSTRING::from(if pin.is_some() {
                    "Unpin from dock"
                } else {
                    "Pin to dock"
                }),
            );
            let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(
                m,
                if running { MF_STRING } else { MF_GRAYED },
                1,
                &HSTRING::from(format!("Quit {name}")),
            );
        });
        let Some(item) = self.d.apps.get(i) else {
            return;
        };
        match (cmd, pin) {
            (1, _) => apps::quit_app(item),
            (2, Some(_)) => {
                // every pin that opens this app, in case it was pinned twice
                let gone: Vec<bool> = self
                    .pinned
                    .iter()
                    .map(|p| p.id.key() == item.key() || p.launch == item.launch)
                    .collect();
                self.change(|s| {
                    let mut g = gone.into_iter();
                    s.pins.retain(|_| !g.next().unwrap_or(false));
                });
            }
            (2, None) => {
                let entry = apps::pin_entry(item);
                self.change(|s| s.pins.push(entry));
            }
            _ => {}
        }
    }

    /// The bar's right-click menu: the things people switch often. The rest is in Settings.
    fn menu(&mut self) {
        use prefs::Act;
        let s = &self.s;
        let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
        let mut acts: Vec<MenuAct> = vec![];
        let cmd = popup_menu(self.here().hwnd, s.top, |root| unsafe {
            let mut add = |m: HMENU, flags: MENU_ITEM_FLAGS, t: &str, act: MenuAct| {
                acts.push(act);
                let _ = AppendMenuW(m, flags, acts.len(), &HSTRING::from(t));
            };
            let sub = |t: &str, m: HMENU| {
                let _ = AppendMenuW(root, MF_POPUP, m.0 as usize, &HSTRING::from(t));
            };
            let line = |m: HMENU| {
                let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
            };
            let style = CreatePopupMenu().unwrap();
            for (i, &st) in Style::ALL.iter().enumerate() {
                if i > 0
                    && crate::glass::params(Style::ALL[i - 1]).is_some()
                        != crate::glass::params(st).is_some()
                {
                    line(style);
                }
                add(
                    style,
                    check(s.style == st),
                    &format!("{} · {}", st.title(), st.description()),
                    MenuAct::Pref(Act::Style(st)),
                );
            }
            let size = CreatePopupMenu().unwrap();
            for z in Size::ALL {
                add(
                    size,
                    check(s.size == z),
                    z.label(),
                    MenuAct::Pref(Act::Size(z)),
                );
            }
            sub("Style", style);
            sub("Size", size);
            line(root);
            add(
                root,
                check(self.capturable),
                "Show in screenshots (freezes the glass)",
                MenuAct::Pref(Act::Screenshots),
            );
            line(root);
            if let Some(r) = &self.d.update {
                let t = format!("Update to {}…", r.version);
                add(root, MF_STRING, &t, MenuAct::Bar(Action::Update));
            }
            add(root, MF_STRING, "Settings…", MenuAct::Bar(Action::Settings));
            add(
                root,
                MF_STRING,
                "About serenebar",
                MenuAct::Pref(Act::About),
            );
            add(root, MF_STRING, "Quit serenebar", MenuAct::Pref(Act::Quit));
        });
        match (cmd as usize).checked_sub(1).and_then(|i| acts.get(i)) {
            Some(&MenuAct::Pref(a)) => self.apply_pref(a),
            Some(&MenuAct::Bar(a)) => self.act(a, 0.0),
            None => {}
        }
    }
}

/// What a menu item does: what the same control in Settings does, or a bar action.
#[derive(Clone, Copy)]
enum MenuAct {
    Pref(prefs::Act),
    Bar(Action),
}

/// Shows a menu at the cursor, opening away from the screen edge the dock is on, and
/// returns the chosen item's id (0 when dismissed).
fn popup_menu(hwnd: HWND, top: bool, build: impl FnOnce(HMENU)) -> i32 {
    unsafe {
        let m = CreatePopupMenu().unwrap();
        build(m);
        let pt = cursor();
        let _ = SetForegroundWindow(hwnd);
        let align = if top { TPM_TOPALIGN } else { TPM_BOTTOMALIGN };
        let cmd = TrackPopupMenu(
            m,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | align,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        )
        .0;
        let _ = DestroyMenu(m);
        cmd
    }
}

fn clipboard_text(hwnd: HWND) -> Option<String> {
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    unsafe {
        OpenClipboard(hwnd).ok()?;
        let text = (|| {
            let h = GetClipboardData(13).ok()?; // CF_UNICODETEXT
            let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(HGLOBAL(h.0));
            Some(s)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// Hands the taskbar back. Dropping the app takes every bar's strip back from the shell.
fn shutdown() {
    drop(APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.take())));
    tray::stop();
    taskbar::taskbar_restore();
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    static TASKBAR_CREATED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    let tb_created =
        *TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });
    let handled = match msg {
        // timers and worker notifications only go to the first bar
        WM_TIMER => with_app(|a| match wp.0 {
            TIMER_TICK => a.update(),
            TIMER_ANIM => a.anim_step(),
            TIMER_SUGGEST => a.suggest_city(),
            // while the bar is visible to capture, new frames would contain it, so keep the last one
            TIMER_GLASS if a.glass_live() => {
                for i in 0..a.bars.len() {
                    // an animation redraws anyway, from the frame polled here
                    if a.bars[i].g.poll_glass() && !a.animating {
                        a.draw(i);
                    }
                }
            }
            _ => {}
        }),
        system::WM_DATA => with_app(|a| a.update()),
        WM_APPBAR => with_app(|a| match wp.0 as u32 {
            // sent to the bars on the monitor where a full-screen app opened or closed
            ABN_FULLSCREENAPP => unsafe {
                let _ = ShowWindow(
                    hwnd,
                    if lp.0 != 0 {
                        SW_HIDE
                    } else {
                        SW_SHOWNOACTIVATE
                    },
                );
            },
            ABN_POSCHANGED => a.dock(),
            _ => {}
        }),
        WM_DISPLAYCHANGE | WM_DPICHANGED => with_app(|a| a.dock()),
        m if m == tb_created => with_app(|a| {
            // Explorer restarted, so hide its new taskbar and register our strips again. Our own
            // tray host broadcasts this too when it starts, with Explorer's taskbar unchanged.
            let tray = taskbar::explorer_tray();
            if tray == a.explorer {
                return;
            }
            a.explorer = tray;
            a.applied.hide_taskbar = None;
            for b in &a.bars {
                appbar(b.hwnd, ABM_NEW, 0, RECT::default());
            }
            a.apply();
        }),
        WM_MOUSEMOVE => with_bar(hwnd, |a, i| unsafe {
            let b = &mut a.bars[i];
            if !b.inside {
                track_leave(hwnd);
                b.inside = true;
            }
            if let Some((icon, x0)) = b.app_press {
                // a held app icon becomes a drag once it moves a few pixels sideways
                if b.app_drag.is_none() && (cursor().x - x0).abs() > (6.0 * b.k()) as i32 {
                    b.app_drag = Some(icon);
                    SetCapture(hwnd);
                }
                if b.app_drag.is_some() {
                    a.drag_app(i);
                }
            }
            let b = &mut a.bars[i];
            if let Some(off) = b.drag {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    b.win.left,
                    cursor().y - off,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                a.draw(i);
            } else {
                let h = b.hit_at(xy(lp)).map(|h| h.0);
                if h != b.hover {
                    b.hover = h;
                    b.name_hovered_app(&a.d);
                    a.draw(i);
                }
                a.update_targets(i);
            }
        }),
        WM_MOUSELEAVE => with_bar(hwnd, |a, i| {
            let b = &mut a.bars[i];
            b.inside = false;
            b.hover = None;
            a.update_targets(i);
            a.draw(i);
        }),
        WM_MOUSEWHEEL => with_bar(hwnd, |a, i| unsafe {
            let (x, y) = xy(lp); // screen coordinates in this message
            let mut p = POINT { x, y };
            let _ = ScreenToClient(hwnd, &mut p);
            if let Some((Action::Volume | Action::Open(Panel::Controls), _)) =
                a.bars[i].hit_at((p.x, p.y))
            {
                let notches = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
                let level = a.d.volume.as_ref().map_or(0.0, |v| v.level);
                a.set_volume(level + notches * 0.04);
            }
        }),
        WM_LBUTTONDOWN => with_bar(hwnd, |a, i| unsafe {
            let (x, y) = xy(lp);
            let b = &mut a.bars[i];
            let on_bar = (0.0..=ui::metrics(a.s.size).h).contains(&(y as f32 / b.k() - b.bar_y));
            match b.hit_at((x, y)) {
                Some((Action::App(icon), _)) => b.app_press = Some((icon, cursor().x)),
                // a press on empty bar starts a drag toward the other edge
                None if on_bar => {
                    b.drag = Some(cursor().y - b.win.top);
                    SetCapture(hwnd);
                }
                _ => {}
            }
        }),
        WM_LBUTTONUP => with_bar(hwnd, |a, i| unsafe {
            let b = &mut a.bars[i];
            b.app_press = None;
            if b.app_drag.take().is_some() {
                let _ = ReleaseCapture();
                let order = a.d.apps.iter().map(|i| i.key().to_string()).collect();
                a.change(|s| s.app_order = order);
                a.animate(); // the dropped icon glides into its slot
            } else if b.drag.take().is_some() {
                let _ = ReleaseCapture();
                let m = b.monitor();
                let top = cursor().y < (m.top + m.bottom) / 2;
                if top != a.s.top {
                    a.change(|s| s.top = top);
                } else {
                    a.draw(i);
                } // back into its place
            } else if let Some((act, frac)) = b.hit_at(xy(lp)) {
                a.act(act, frac);
            }
        }),
        WM_RBUTTONUP => with_bar(hwnd, |a, i| match a.bars[i].hit_at(xy(lp)) {
            Some((Action::App(icon), _)) => a.app_menu(icon),
            _ => a.menu(),
        }),
        // the shell recomputes the work area, and so the size of maximized windows, from this
        WM_WINDOWPOSCHANGED => {
            appbar(hwnd, ABM_WINDOWPOSCHANGED, 0, RECT::default());
            None
        }
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        // nothing of ours closes a bar; whoever does wants serenebar gone
        WM_CLOSE => {
            unsafe { PostQuitMessage(0) };
            Some(())
        }
        WM_ENDSESSION if wp.0 != 0 => {
            shutdown();
            Some(())
        }
        _ => None,
    };
    if handled.is_some() {
        LRESULT(0)
    } else {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }
}

/// Panels close when they lose focus, on Escape, or from their close button.
extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let handled = match msg {
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => with_app(|a| a.close_panel()),
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => with_app(|a| a.close_panel()),
        WM_LBUTTONDOWN => with_app(|a| a.panel_press(lp)),
        WM_MOUSEMOVE => with_app(|a| a.panel_move(lp)),
        WM_MOUSELEAVE => with_app(|a| a.leave_popup()),
        WM_LBUTTONUP => with_app(|a| a.panel_click(lp)),
        WM_RBUTTONUP => with_app(|a| {
            if let Some((panel::Act::TrayIcon(i), _)) = a.panel_hit(lp) {
                a.tray_click(i, true)
            }
        }),
        WM_CLOSE => with_app(|a| a.close_panel()), // Alt+F4
        _ => None,
    };
    if handled.is_some() {
        LRESULT(0)
    } else {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }
}

/// The settings window: dragged by the empty part of its header, closed with its button,
/// Escape or Alt+F4.
extern "system" fn prefs_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        let (x, y) = xy(lp); // screen coordinates in this message
        let mut p = POINT { x, y };
        unsafe {
            let _ = ScreenToClient(hwnd, &mut p);
        }
        let caption = with_app(|a| {
            a.prefs.as_ref().is_some_and(|w| {
                let (x, y) = (p.x as f32 / w.g.k, p.y as f32 / w.g.k);
                y < prefs::HEADER && ui::hit_test(&w.hits, x, y).is_none()
            })
        })
        .unwrap_or(false);
        return LRESULT(if caption { HTCAPTION } else { HTCLIENT } as isize);
    }
    let handled = match msg {
        WM_LBUTTONDOWN => with_app(|a| a.prefs_press(lp)),
        WM_MOUSEMOVE => with_app(|a| a.prefs_move(lp)),
        WM_MOUSELEAVE => with_app(|a| a.leave_popup()),
        WM_LBUTTONUP => with_app(|a| a.prefs_click(lp)),
        WM_CHAR => with_app(|a| a.prefs_char(wp.0 as u32)),
        WM_KEYDOWN => with_app(|a| a.prefs_key(wp.0 as u16)),
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => with_app(|a| a.commit_field()),
        WM_CLOSE => with_app(|a| a.close_prefs()),
        _ => None,
    };
    if handled.is_some() {
        LRESULT(0)
    } else {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }
}

pub fn main() -> Result<()> {
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\serenebar"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        let hinst = GetModuleHandleW(None)?;
        let arrow = LoadCursorW(None, IDC_ARROW)?;
        let icon = LoadIconW(hinst, PCWSTR(1 as _)).unwrap_or_default(); // resource 1, see build.rs
        let class = |name, proc_: WNDPROC| WNDCLASSW {
            lpfnWndProc: proc_,
            hInstance: hinst.into(),
            lpszClassName: name,
            hCursor: arrow,
            hIcon: icon,
            ..Default::default()
        };
        RegisterClassW(&class(w!("serenebar"), Some(wndproc)));
        RegisterClassW(&class(w!("serenebar-panel"), Some(panel_proc)));
        RegisterClassW(&class(w!("serenebar-settings"), Some(prefs_proc)));
        let hwnd = bar_window()?;

        let s = Settings::load(apps::taskbar_pins);
        if !Settings::path().exists() {
            s.save();
        }
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            taskbar::taskbar_restore();
            default_hook(info)
        }));

        let app = App {
            bars: vec![Bar::new(hwnd, screens(false)[0])?],
            at: 0,
            d: ui::Data::default(),
            pinned: vec![],
            app_order: vec![],
            start_left: None,
            cpu_prev: (0, 0),
            net_prev: None,
            art_id: 0,
            tx: controls::spawn_winrt(hwnd),
            feeds: feeds::spawn_feeds(hwnd),
            applied: Applied::default(),
            explorer: taskbar::explorer_tray(),
            started: Instant::now(),
            animating: false,
            panel: None,
            capturable: false,
            prefs: None,
            installer: None,
            installing: false,
            s,
        };
        APP.with(|a| *a.borrow_mut() = Some(app));
        with_app(|a| {
            a.apply();
            a.animate();
        });
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, TIMER_TICK, 1000, None);
        SetTimer(hwnd, TIMER_GLASS, 33, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        shutdown();
        Ok(())
    }
}
