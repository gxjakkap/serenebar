//! All units are DIPs.
use crate::dates::LocalTime;
use crate::dock::MAG;
use crate::gfx::{rgba, Align, Gfx, C};
use crate::limits::Limit;
use crate::settings::{
    dark_text_on, Control, Settings, Size, Slot, Style, Variant, Widget, ACCENTS, ACCENT_TEXT,
    DANGER_TEXT, GROUPS,
};
use crate::tray::TrayIcon;
use crate::weather::Weather;
use std::f32::consts::PI;
use windows::Foundation::Numerics::Matrix3x2;

pub struct Theme {
    pub text: C,
    pub sub: C,
    pub tile: C,
    pub tile_hover: C,
    pub tile_border: C,
    pub bar: Option<C>,     // solid fill, or the tint laid over glass
    pub rim: [(f32, C); 3], // bar outline gradient stops, top to bottom
    pub chip: C,            // small round buttons
    pub dark: bool,
}

pub fn theme(s: Style) -> Theme {
    let w = |a| rgba(0xFFFFFF, a);
    let b = |a| rgba(0x000000, a);
    let dark = |bar: Option<C>, tile: f32, border: f32| Theme {
        text: pal(true).text,
        sub: pal(true).sub,
        tile: w(tile),
        tile_hover: w(tile + 0.06),
        tile_border: w(border),
        bar,
        rim: [(0.0, w(0.34)), (0.5, w(0.08)), (1.0, w(0.20))],
        chip: w(0.10),
        dark: true,
    };
    let light = |bar: Option<C>, text: u32, sub: u32| Theme {
        text: rgba(text, 1.0),
        sub: rgba(sub, 0.78), // 4.7:1 or better on the light bars
        tile: b(0.035),
        tile_hover: b(0.07),
        tile_border: b(0.07),
        bar,
        rim: [(0.0, b(0.06)), (0.5, b(0.08)), (1.0, b(0.12))],
        chip: b(0.06),
        dark: false,
    };
    match s {
        Style::Lagoon => dark(Some(rgba(0x14141C, 0.20)), 0.035, 0.13), // almost no tint
        Style::Frost => dark(Some(rgba(0x1E161E, 0.34)), 0.0, 0.10),
        Style::Dusk => dark(Some(rgba(0x0B0D16, 0.74)), 0.04, 0.10), // a deep night tint
        Style::Ember => dark(Some(rgba(0x2B1406, 0.46)), 0.03, 0.12),
        Style::Tide => dark(Some(rgba(0x05222B, 0.46)), 0.03, 0.12),
        Style::Mist => Theme {
            rim: [(0.0, w(0.70)), (0.5, w(0.25)), (1.0, w(0.45))],
            ..light(Some(rgba(0xF4F6FA, 0.55)), 0x1D1D1F, 0x3C3C43)
        },
        Style::Obsidian => Theme {
            rim: [(0.0, w(0.14)), (0.5, w(0.06)), (1.0, w(0.10))],
            ..dark(Some(rgba(0x0C0C0E, 0.97)), 0.05, 0.09)
        },
        Style::Slate => Theme {
            rim: [(0.0, w(0.16)), (0.5, w(0.06)), (1.0, w(0.10))],
            ..dark(Some(rgba(0x1E2530, 0.97)), 0.05, 0.08)
        },
        Style::Porcelain => light(Some(rgba(0xF6F6F8, 0.97)), 0x1D1D1F, 0x3C3C43),
        Style::Sand => light(Some(rgba(0xEFE7DA, 0.97)), 0x2B2418, 0x4A3F2C),
        Style::Bare => Theme {
            rim: [(0.0, w(0.0)), (0.5, w(0.0)), (1.0, w(0.0))],
            ..dark(None, 0.0, 0.14)
        },
    }
}

#[derive(Clone, Copy)]
pub struct Metrics {
    pub h: f32,
    pub pad: f32,
    pub gap: f32,
    pub r_bar: f32,
    pub r_tile: f32,
    pub icon: f32,
    pub edge: f32,
    pub k: f32,
}

pub fn metrics(size: Size) -> Metrics {
    let k = size.scale();
    Metrics {
        h: 56.0 * k,
        pad: 5.0 * k,
        gap: 6.0 * k,
        r_bar: 19.0 * k,
        r_tile: 13.0 * k,
        icon: 34.0 * k,
        edge: 6.0 * k,
        k,
    }
}

impl Metrics {
    /// Corner radius of the bar: square when it spans the screen.
    pub fn bar_radius(&self, full: bool) -> f32 {
        if full {
            0.0
        } else {
            self.r_bar
        }
    }
}

/// Icons are made this many pixels wide: enough for a Large dock icon at full magnification.
pub fn icon_px(k: f32) -> u32 {
    (metrics(Size::L).icon * MAG * k).ceil() as u32
}

/// Room the magnified icons need beyond the bar.
pub fn head(m: &Metrics) -> f32 {
    (MAG - 1.0) * m.icon + 6.0 * m.k
}

/// Room beyond the magnified icons for the name over the hovered one. DIPs at scale 1.
pub const TIP_ROOM: f32 = 16.0;

/// What the name over a hovered control says, for the controls that are only a glyph.
/// App names come from the shell, so the caller looks those up.
pub fn tip(act: Action, d: &Data) -> Option<String> {
    let radio = |name: &str| match d.radio(act) {
        Some(true) => format!("{name} on"),
        Some(false) => format!("{name} off"),
        None => name.to_string(),
    };
    let playing = d.media.as_ref().is_some_and(|m| m.playing);
    Some(match act {
        Action::Start => "Start".into(),
        Action::Open(Panel::Tray) => "Tray icons".into(),
        Action::ShowDesktop => "Show desktop".into(),
        Action::Notifications => "Notifications".into(),
        Action::Wifi => radio("Wi-Fi"),
        Action::Bluetooth => radio("Bluetooth"),
        Action::Vpn => radio("VPN"),
        Action::Airplane => radio("Airplane mode"),
        Action::Hotspot => radio("Mobile hotspot"),
        Action::Prev => "Previous".into(),
        Action::Toggle if playing => "Pause".into(),
        Action::Toggle => "Play".into(),
        Action::Next => "Next".into(),
        Action::TaskManager => "Task Manager".into(),
        Action::Mic if d.mic == Some(true) => "Microphone muted".into(),
        Action::Mic => "Mute microphone".into(),
        Action::Mute if d.volume.as_ref().is_some_and(|v| v.muted) => "Sound muted".into(),
        Action::Mute => "Mute sound".into(),
        Action::Theme if d.light == Some(false) => "Light mode".into(),
        Action::Theme => "Dark mode".into(),
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Panel {
    Weather,
    Calendar,
    Claude,
    Codex,
    Antigravity,
    Cursor,
    Controls,
    About,
    Tray,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Start,
    App(usize),
    Prev,
    Toggle,
    Next,
    TaskManager,
    Open(Panel),
    Wifi,
    Bluetooth,
    Vpn,
    Airplane,
    Hotspot,
    Mute, // the speakers: deafen
    Mic,
    Theme,
    Volume,
    AppVolume(usize), // an app's slider in the quick controls panel
    AppMute(usize),
    Settings,
    Language,
    ShowDesktop,
    QuickSettings,
    SystemSettings,
    SettingsFolder,
    Update,   // opens the newer release's page
    DeepSeek, // opens DeepSeek's usage page
    Gmail,
    Notifications,
}

#[derive(Default)]
pub struct Data {
    pub cpu: f32,
    pub mem: f32,
    pub battery: Option<(f32, bool)>,
    pub apps: Vec<crate::apps::AppItem>,
    pub media: Option<crate::controls::Media>,
    pub art_tint: Option<C>,
    pub art: Option<std::sync::Arc<Vec<u8>>>, // the cover, encoded; drawn from each bar's cache as "art"
    pub weather: Option<Weather>,
    pub weather_w: f32, // label/city column plus temperature, measured at scale 1
    pub events: Vec<crate::calendar::Event>,
    pub claude: Option<Vec<Limit>>,
    pub codex: Option<Vec<Limit>>,
    pub antigravity: Option<Vec<Limit>>,
    pub deepseek: Option<Vec<crate::limits::Balance>>,
    pub cursor: Option<Vec<Limit>>,
    pub gmail: Option<Result<u32, &'static str>>, // unread, or what to show instead
    pub google: bool,                             // signed in with Google
    pub update: Option<crate::update::Release>,   // a newer release than this build
    pub update_note: &'static str,                // how a check from Settings went
    pub volume: Option<crate::controls::Volume>,
    pub app_volumes: Vec<crate::controls::AppVolume>, // read only while the controls panel is open
    pub mic: Option<bool>,                            // muted; None without a microphone
    pub net: Option<(f64, f64)>,                      // bytes per second down and up
    pub light: Option<bool>,                          // Windows apps are in light mode
    pub wifi: Option<bool>,
    pub vpn: Option<bool>,
    pub airplane: Option<bool>,
    pub hotspot: Option<bool>,
    pub lang: Option<(String, String)>,
    pub bluetooth: Option<bool>,
    pub tray: Vec<TrayIcon>,
    pub now: i64, // unix seconds
    pub time: LocalTime,
}

pub struct View<'a> {
    pub s: &'a Settings,
    pub th: Theme,
    pub m: Metrics,
    pub accent: C,
    pub hover: Option<Action>,
    pub mag: &'a [f32], // per-icon scale, Start tile first. Empty means 1.0 everywhere.
    pub intro: f32,     // seconds since launch
    pub slide: &'a [f32], // per app: DIPs it's still sliding from, after a reorder
    pub lifted: Option<usize>, // app being dragged, drawn above the rest
    pub bar_x: f32, // where the bar sits inside the window. The magnification room is beside it.
    pub bar_y: f32,
    pub tip: Option<String>, // the name shown over the hovered control
}

pub fn accent(s: &Settings) -> C {
    rgba(ACCENTS[s.accent].0, 1.0)
}

/// The accent as text on the panels and the settings window.
pub fn accent_text(s: &Settings, dark: bool) -> C {
    let (l, d) = ACCENT_TEXT[s.accent];
    rgba(if dark { d } else { l }, 1.0)
}

/// Red text for actions like Quit.
pub fn danger_text(dark: bool) -> C {
    rgba(if dark { DANGER_TEXT.1 } else { DANGER_TEXT.0 }, 1.0)
}

/// Text and glyphs on a fill of `c`: white, or near-black on the light accents.
pub fn on_fill(c: C) -> C {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let dark = dark_text_on(b(c.r) << 16 | b(c.g) << 8 | b(c.b));
    rgba(if dark { 0x1D1D1F } else { 0xFFFFFF }, 1.0)
}

/// Smallest text on the bar, whatever the size setting.
const MIN_TEXT: f32 = 10.0;

/// Bar text: `base` DIPs scaled with the dock, never below `MIN_TEXT`.
pub fn font(k: f32, base: f32) -> f32 {
    (base * k).max(MIN_TEXT)
}

/// A clickable box and what clicking it does.
pub type Hit<A> = (f32, f32, f32, f32, A);

/// What's under (x, y), and how far across its box the point is (0 to 1).
pub fn hit_test<A: Copy>(hits: &[Hit<A>], x: f32, y: f32) -> Option<(A, f32)> {
    hits.iter()
        .find(|h| x >= h.0 && x < h.0 + h.2 && y >= h.1 && y < h.1 + h.3)
        .map(|h| (h.4, (x - h.0) / h.2))
}

/// Where the box for `act` is, as (x, width).
pub fn hit_span<A: PartialEq>(hits: &[Hit<A>], act: A) -> Option<(f32, f32)> {
    hits.iter().find(|h| h.4 == act).map(|h| (h.0, h.2))
}

/// Colours of the panels and the settings window.
pub struct Pal {
    pub bg: C,
    pub card: C,
    pub text: C,
    pub sub: C,
    pub border: C,
}

/// The faint fill under a control the cursor is over.
pub fn wash(p: &Pal) -> C {
    C { a: 0.08, ..p.text }
}

/// The ✕ in a card's top right corner.
pub fn close_button<A>(
    g: &Gfx,
    p: &Pal,
    cx: f32,
    cy: f32,
    act: A,
    hovered: bool,
    hits: &mut Vec<Hit<A>>,
) {
    if hovered {
        g.fill_circle(cx, cy, 15.0, wash(p));
    }
    g.line(cx - 6.0, cy - 6.0, cx + 6.0, cy + 6.0, p.sub, 1.8);
    g.line(cx + 6.0, cy - 6.0, cx - 6.0, cy + 6.0, p.sub, 1.8);
    hits.push((cx - 16.0, cy - 16.0, 32.0, 32.0, act));
}

pub fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            bg: rgba(0x1C1C1E, 0.98),
            card: rgba(0x2C2C2E, 1.0),
            text: rgba(0xF5F5F7, 1.0),
            sub: rgba(0xEBEBF5, 0.6),
            border: rgba(0xFFFFFF, 0.10),
        }
    } else {
        Pal {
            bg: rgba(0xF2F2F7, 0.98),
            card: rgba(0xFFFFFF, 1.0),
            text: rgba(0x1D1D1F, 1.0),
            sub: rgba(0x3C3C43, 0.78), // 5.3:1 on bg, 5.6:1 on card
            border: rgba(0x000000, 0.08),
        }
    }
}

/// Width of a tile, or None when it has nothing to show.
fn tile_w(w: Widget, d: &Data, v: &View) -> Option<f32> {
    let k = v.m.k;
    Some(match w {
        Widget::Start => icons_w(v, 0..1),
        Widget::Apps if !d.apps.is_empty() => icons_w(v, 1..d.apps.len() + 1),
        Widget::Media => match v.s.variant(w) {
            Variant::ExtraLarge => 236.0 * k,
            Variant::Medium => 184.0 * k,
            Variant::Small => 46.0 * k,
            _ => 196.0 * k,
        },
        Widget::Weather => (WEATHER_PAD + d.weather_w).max(150.0) * k,
        Widget::Date if v.s.variant(w) == Variant::DayNumber => 46.0 * k,
        Widget::Date => 128.0 * k, // room for "September 30"
        Widget::Clock => match v.s.variant(w) {
            Variant::Small if v.s.clock24 => 64.0 * k,
            Variant::Small => 84.0 * k,
            Variant::TearOff => 56.0 * k,
            _ => 100.0 * k,
        },
        Widget::Claude | Widget::Codex | Widget::Antigravity | Widget::Cursor
            if d.limits(ai_panel(w)).is_some() =>
        {
            match v.s.variant(w) {
                Variant::Medium => 144.0 * k,
                Variant::Small => 88.0 * k,
                _ => 196.0 * k,
            }
        }
        Widget::Controls => {
            let w: f32 = v.s.controls.iter().map(|&c| control_w(c, d)).sum();
            if w == 0.0 {
                return None;
            }
            (w + 8.0) * k
        }
        Widget::Lang if d.lang.is_some() => 46.0 * k,
        Widget::Tray | Widget::Desktop | Widget::Theme | Widget::Notifications => 24.0 * k,
        Widget::System => 96.0 * k,
        Widget::Mic => 68.0 * k,
        Widget::Network => 92.0 * k,
        Widget::Progress => 140.0 * k,
        Widget::DeepSeek => 132.0 * k,
        Widget::Gmail => 120.0 * k,
        Widget::Battery if d.battery.is_some() => 56.0 * k,
        Widget::Apps
        | Widget::Claude
        | Widget::Codex
        | Widget::Antigravity
        | Widget::Cursor
        | Widget::Lang
        | Widget::Battery => return None,
    })
}

/// Every tile with its group, x and width. Each `|` in the widget list starts the next of the
/// `GROUPS` groups. In a pill the groups sit side by side, a wider gap apart. Full width pins
/// them to the left, centre and right of the screen, the way the Windows taskbar lays out.
pub fn layout(d: &Data, v: &View, full_w: Option<f32>) -> Vec<(Widget, usize, f32, f32)> {
    let mut groups: [Vec<(Widget, f32)>; GROUPS] = Default::default();
    let mut gi = 0;
    for slot in &v.s.widgets {
        match *slot {
            Slot::Break => gi = (gi + 1).min(GROUPS - 1),
            Slot::Tile(w) => {
                if let Some(width) = tile_w(w, d, v) {
                    groups[gi].push((w, width));
                }
            }
        }
    }
    let span = |g: &[(Widget, f32)]| {
        g.iter().map(|t| t.1).sum::<f32>() + v.m.gap * g.len().saturating_sub(1) as f32
    };
    let starts: [f32; GROUPS] = match full_w {
        Some(w) => crate::dock::full_width_starts(
            w,
            v.m.pad,
            GROUP_GAP * v.m.k,
            groups.each_ref().map(|g| span(g)),
        ),
        None => {
            let mut x = v.m.pad;
            groups.each_ref().map(|g| {
                let s = x;
                if !g.is_empty() {
                    x += span(g) + GROUP_GAP * v.m.k;
                }
                s
            })
        }
    };
    let mut out = vec![];
    for (gi, (g, start)) in groups.iter().zip(starts).enumerate() {
        let mut x = start;
        for (id, w) in g {
            out.push((*id, gi, x, *w));
            x += w + v.m.gap;
        }
    }
    out
}

const GROUP_GAP: f32 = 18.0;

/// Width of the pill.
pub fn content_width(d: &Data, v: &View) -> f32 {
    layout(d, v, None)
        .last()
        .map_or(2.0 * v.m.pad, |t| t.2 + t.3 + v.m.pad)
}

/// Unmagnified centres of every dock icon (Start first, NaN when its tile is off) and the
/// x ranges of the icon tiles, in window DIPs.
pub fn icon_layout(d: &Data, v: &View, full_w: Option<f32>) -> (Vec<(f32, f32)>, Vec<f32>) {
    let mut centres = vec![f32::NAN; d.apps.len() + 1];
    let mut ranges = vec![];
    for (w, _, x, width) in layout(d, v, full_w) {
        let icons = match w {
            Widget::Start => 0..1,
            Widget::Apps => 1..d.apps.len() + 1,
            _ => continue,
        };
        for j in icons.clone() {
            centres[j] =
                x + ICON_LEAD * v.m.k + (j - icons.start) as f32 * icon_step(&v.m) + v.m.icon / 2.0;
        }
        ranges.push((x, x + width));
    }
    (ranges, centres)
}

/// Room around each dock icon, and before the first one, in DIPs at size M.
const ICON_GAP: f32 = 8.0;
const ICON_LEAD: f32 = 2.0;

/// Distance from one unmagnified dock icon to the next.
pub fn icon_step(m: &Metrics) -> f32 {
    m.icon + ICON_GAP * m.k
}

/// Width of a run of dock icons at their current magnification.
fn icons_w(v: &View, range: std::ops::Range<usize>) -> f32 {
    range
        .map(|j| v.m.icon * v.mag.get(j).copied().unwrap_or(1.0) + ICON_GAP * v.m.k)
        .sum::<f32>()
        + 2.0 * ICON_LEAD * v.m.k
}

/// On launch the pill grows out from the centre, then the tiles pop in one after another.
const GROW: f32 = 0.45;
const POP_DELAY: f32 = 0.15;
const POP_STAGGER: f32 = 0.06;
const POP: f32 = 0.45;

/// Seconds the launch animation runs with `tiles` tiles.
pub fn intro_end(tiles: usize) -> f32 {
    (POP_DELAY + POP_STAGGER * tiles.saturating_sub(1) as f32 + POP).max(GROW)
}

fn ease_out_back(x: f32) -> f32 {
    let (c1, c3) = (1.70158, 2.70158);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

/// Scale about a point, then move the bar to its place in the window.
fn about(s: f32, cx: f32, cy: f32, v: &View) -> Matrix3x2 {
    Matrix3x2 {
        M11: s,
        M12: 0.0,
        M21: 0.0,
        M22: s,
        M31: cx * (1.0 - s) + v.bar_x,
        M32: cy * (1.0 - s) + v.bar_y,
    }
}

/// Bar background. `glass` is true when gfx.prepare_glass succeeded this frame.
pub fn draw_bar(g: &Gfx, v: &View, w: f32, glass: bool) {
    let full = v.s.full;
    g.set_transform(about(1.0, 0.0, 0.0, v));
    let h = v.m.h;
    let r = v.m.bar_radius(full);
    let grow = if full {
        1.0
    } else {
        1.0 - (1.0 - (v.intro / GROW).clamp(0.0, 1.0)).powi(3)
    };
    let bw = w * (0.25 + 0.75 * grow);
    let x = (w - bw) / 2.0;
    if glass {
        g.fill_glass(x, 0.0, bw, h, r);
    }
    if let Some(mut c) = v.th.bar {
        if crate::glass::params(v.s.style).is_some() && !glass {
            c.a = c.a.max(0.82);
        } // without a capture, a darker tint keeps the text readable
        g.fill_rr(x, 0.0, bw, h, r, c);
    }
    if full {
        // a single hairline on the side facing the desktop
        let y = if v.s.top { h - 1.0 } else { 0.0 };
        g.fill_rr(0.0, y, w, 1.0, 0.0, v.th.rim[0].1);
    } else {
        g.stroke_rr_grad(x, 0.0, bw, h, r, &v.th.rim, 1.0);
        if glass {
            g.stroke_rr(
                x + 1.0,
                1.0,
                bw - 2.0,
                h - 2.0,
                r - 1.0,
                rgba(0xFFFFFF, 0.05),
                1.0,
            );
        }
    }
}

pub fn draw_tiles(g: &Gfx, v: &View, d: &Data, full_w: Option<f32>) -> Vec<Hit<Action>> {
    let m = v.m;
    let (y, h) = (m.pad, m.h - 2.0 * m.pad);
    let mut hits = vec![];
    let tiles = layout(d, v, full_w);
    for (i, &(widget, group, x, w)) in tiles.iter().enumerate() {
        if full_w.is_none() && i > 0 && tiles[i - 1].1 != group {
            // a faint divider in the middle of the gap between groups
            let dx = x - GROUP_GAP * m.k / 2.0;
            g.set_transform(about(1.0, 0.0, 0.0, v));
            g.fill_rr(
                dx - 0.5,
                y + h * 0.25,
                1.0,
                h * 0.5,
                0.5,
                dim(v.th.text, 0.14),
            );
        }
        let p = ((v.intro - POP_DELAY - POP_STAGGER * i as f32) / POP).clamp(0.0, 1.0);
        if p > 0.0 {
            g.set_transform(about(ease_out_back(p), x + w / 2.0, y + h / 2.0, v));
            match widget {
                Widget::Start => icons(g, v, d, x, y, h, 0..1, &mut hits),
                Widget::Apps => icons(g, v, d, x, y, h, 1..d.apps.len() + 1, &mut hits),
                Widget::Media => match v.s.variant(widget) {
                    Variant::ExtraLarge | Variant::Medium => {
                        media_card(g, v, d, x, y, w, h, &mut hits)
                    }
                    Variant::Small => media_mini(g, v, d, x, y, w, h, &mut hits),
                    _ => media(g, v, d, x, y, w, h, &mut hits),
                },
                Widget::Weather => weather(g, v, d, x, y, w, h, &mut hits),
                Widget::Date => date(g, v, d, x, y, w, h, &mut hits),
                Widget::Clock => match v.s.variant(widget) {
                    Variant::Small => small_clock(g, v, d, x, y, w, h, &mut hits),
                    Variant::TearOff => dateclock(g, v, d, x, y, w, h, &mut hits),
                    _ => clock(g, v, d, x, y, w, h, &mut hits),
                },
                Widget::Claude | Widget::Codex | Widget::Antigravity | Widget::Cursor => {
                    match v.s.variant(widget) {
                        Variant::Medium | Variant::Small => {
                            ai_compact(g, v, d, widget, x, y, w, h, &mut hits)
                        }
                        _ => ai(g, v, d, ai_panel(widget), x, y, w, h, &mut hits),
                    }
                }
                Widget::Controls => controls(g, v, d, x, y, w, h, &mut hits),
                Widget::Lang => lang(g, v, d, x, y, w, h, &mut hits),
                Widget::Tray => tray(g, v, x, y, w, h, &mut hits),
                Widget::Desktop => desktop(g, v, x, y, w, h, &mut hits),
                Widget::System => system(g, v, d, x, y, w, h, &mut hits),
                Widget::Battery => battery(g, v, d, x, y, w, h),
                Widget::Mic => mic(g, v, d, x, y, w, h, &mut hits),
                Widget::Network => network(g, v, d, x, y, w, h, &mut hits),
                Widget::Theme => theme_toggle(g, v, x, y, w, h, &mut hits),
                Widget::Progress => progress(g, v, d, x, y, w, h, &mut hits),
                Widget::DeepSeek => deepseek(g, v, d, x, y, w, h, &mut hits),
                Widget::Gmail => gmail(g, v, d, x, y, w, h, &mut hits),
                Widget::Notifications => notifications(g, v, x, y, w, h, &mut hits),
            }
        }
    }
    g.set_transform(about(1.0, 0.0, 0.0, v));
    draw_tip(g, v, &hits, full_w.unwrap_or_else(|| content_width(d, v)));
    g.set_transform(Matrix3x2::identity());
    hits
}

/// The name over the hovered control, on the side away from the screen edge, kept over the bar.
fn draw_tip(g: &Gfx, v: &View, hits: &[Hit<Action>], bar_w: f32) {
    let (Some(text), Some(act)) = (&v.tip, v.hover) else {
        return;
    };
    let Some(&(hx, hy, hw, hh, _)) = hits.iter().find(|h| h.4 == act) else {
        return;
    };
    let (k, p) = (v.m.k, pal(v.th.dark));
    let f = font(k, 12.0);
    let (tw, th) = (g.measure(text, f, 600) + 20.0 * k, 24.0 * k);
    let tx = (hx + hw / 2.0 - tw / 2.0).clamp(0.0, (bar_w - tw).max(0.0));
    let ty = if v.s.top {
        hy + hh + 6.0 * k
    } else {
        hy - 6.0 * k - th
    };
    g.fill_rr(tx, ty, tw, th, th / 2.0, p.bg);
    g.stroke_rr(tx, ty, tw, th, th / 2.0, p.border, 1.0);
    g.text(text, tx, ty, tw, th, f, 600, p.text, Align::Center);
}

fn tile(g: &Gfx, v: &View, x: f32, y: f32, w: f32, h: f32, hovered: bool) {
    let r = v.m.r_tile;
    g.fill_rr(
        x,
        y,
        w,
        h,
        r,
        if hovered { v.th.tile_hover } else { v.th.tile },
    );
    g.stroke_rr(x, y, w, h, r, v.th.tile_border, 1.0);
}

fn white(a: f32) -> C {
    rgba(0xFFFFFF, a)
}

fn dim(mut c: C, f: f32) -> C {
    c.a *= f;
    c
}

pub fn media_glyph(g: &Gfx, act: Action, playing: bool, cx: f32, cy: f32, s: f32, c: C) {
    match act {
        Action::Toggle if playing => {
            g.fill_rr(cx - s * 0.85, cy - s, s * 0.6, s * 2.0, s * 0.2, c);
            g.fill_rr(cx + s * 0.25, cy - s, s * 0.6, s * 2.0, s * 0.2, c);
        }
        Action::Toggle => g.poly(
            &[(cx - s * 0.6, cy - s), (cx + s, cy), (cx - s * 0.6, cy + s)],
            c,
        ),
        Action::Prev => {
            let s = s * 0.8;
            g.fill_rr(cx - s, cy - s, s * 0.35, s * 2.0, s * 0.12, c);
            g.poly(&[(cx + s, cy - s), (cx - s * 0.6, cy), (cx + s, cy + s)], c);
        }
        _ => {
            let s = s * 0.8;
            g.fill_rr(cx + s * 0.65, cy - s, s * 0.35, s * 2.0, s * 0.12, c);
            g.poly(&[(cx - s, cy - s), (cx + s * 0.6, cy), (cx - s, cy + s)], c);
        }
    }
}

/// The music pill takes its colour from the album art.
fn media(g: &Gfx, v: &View, d: &Data, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    let base = d.art_tint.map(deepen).unwrap_or(rgba(0x7A3B2E, 1.0));
    let lift = C {
        r: (base.r + 0.08).min(1.0),
        g: (base.g + 0.08).min(1.0),
        b: (base.b + 0.08).min(1.0),
        a: 1.0,
    };
    g.fill_rr_grad(x, y, w, h, v.m.r_tile, &[(0.0, lift), (1.0, base)], false);
    g.stroke_rr(x, y, w, h, v.m.r_tile, white(0.10), 1.0);

    let (line, playing) = match &d.media {
        Some(m) if !m.artist.is_empty() => (format!("{} · {}", m.title, m.artist), m.playing),
        Some(m) => (m.title.clone(), m.playing),
        None => ("Nothing playing".to_string(), false),
    };
    g.text(
        &line,
        x + 10.0 * k,
        y + 5.0 * k,
        w - 20.0 * k,
        15.0 * k,
        font(k, 11.0),
        600,
        white(0.95),
        Align::Center,
    );
    let cy = y + h * 0.68;
    let fade = if d.media.is_some() { 1.0 } else { 0.45 };
    for (i, act) in [Action::Prev, Action::Toggle, Action::Next]
        .into_iter()
        .enumerate()
    {
        let cx = x + w / 2.0 + (i as f32 - 1.0) * 30.0 * k;
        if v.hover == Some(act) {
            g.fill_circle(cx, cy, 11.0 * k, white(0.16));
        }
        media_glyph(g, act, playing, cx, cy, 4.2 * k, white(0.95 * fade));
        hits.push((cx - 14.0 * k, y + h * 0.4, 28.0 * k, h * 0.6, act));
    }
}

/// Darkens a cover's average colour into a tile background that white text reads on.
fn deepen(c: C) -> C {
    let m = c.r.max(c.g).max(c.b).max(0.01);
    let f = 0.48 / m;
    C {
        r: (c.r * f).min(1.0),
        g: (c.g * f).min(1.0),
        b: (c.b * f).min(1.0),
        a: 1.0,
    }
}

/// The weather tile's fixed room: 10 left margin, 8 between columns, 4 before the 32 icon slot.
const WEATHER_PAD: f32 = 54.0;

fn weather(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    let r = v.m.r_tile;
    let Some(wx) = &d.weather else {
        let has_city = !v.s.city.is_empty() || v.s.lat.is_some();
        let act = if has_city {
            Action::Open(Panel::Weather)
        } else {
            Action::Settings
        };
        tile(g, v, x, y, w, h, v.hover == Some(act));
        let msg = if has_city {
            "Loading weather…"
        } else {
            "Add a city in Settings"
        };
        g.text(
            msg,
            x + 8.0 * k,
            y,
            w - 16.0 * k,
            h,
            font(k, 10.0),
            500,
            v.th.sub,
            Align::Center,
        );
        hits.push((x, y, w, h, act));
        return;
    };
    let (top, bottom) = if wx.day {
        (0x1375CE, 0x245FB7) // white is 4.7:1 on the lighter top
    } else {
        (0x2B3A67, 0x141B33)
    };
    g.fill_rr_grad(
        x,
        y,
        w,
        h,
        r,
        &[(0.0, rgba(top, 1.0)), (1.0, rgba(bottom, 1.0))],
        false,
    );
    g.stroke_rr(x, y, w, h, r, white(0.14), 1.0);
    let (label, icon) = crate::weather::wmo(wx.code, wx.day);
    let temp = format!("{:.0}°", wx.temp);
    let tw = g.measure(&temp, font(k, 21.0), 300);
    let tx = x + w - 36.0 * k - tw;
    let lw = tx - 8.0 * k - (x + 10.0 * k);
    g.text(
        label,
        x + 10.0 * k,
        y + 6.0 * k,
        lw,
        15.0 * k,
        font(k, 11.0),
        600,
        white(1.0),
        Align::Left,
    );
    g.text(
        &wx.city,
        x + 10.0 * k,
        y + 21.0 * k,
        lw,
        15.0 * k,
        font(k, 10.0),
        500,
        white(1.0),
        Align::Left,
    );
    g.text(
        &temp,
        tx,
        y,
        tw + 1.0,
        h,
        font(k, 21.0),
        300,
        white(1.0),
        Align::Left,
    );
    g.text(
        icon,
        x + w - 32.0 * k,
        y + h * 0.38,
        30.0 * k,
        h * 0.62,
        17.0 * k,
        400,
        white(1.0),
        Align::Center,
    );
    hits.push((x, y, w, h, Action::Open(Panel::Weather)));
}

fn clock(g: &Gfx, v: &View, d: &Data, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    let act = Action::Open(Panel::Calendar);
    tile(g, v, x, y, w, h, v.hover == Some(act));
    let t = d.time;
    let hm = format!(
        "{}:{:02}",
        if v.s.clock24 {
            t.hour
        } else {
            crate::dates::hour12(t.hour)
        },
        t.minute
    );
    let secs = format!(":{:02}", t.second);
    let ampm = if v.s.clock24 {
        ""
    } else if t.hour < 12 {
        "AM"
    } else {
        "PM"
    };
    let (big, small) = (font(k, 17.0), font(k, 10.0));
    let (wb, ws) = (g.measure(&hm, big, 600), g.measure(&secs, small, 500));
    let wa = if ampm.is_empty() {
        0.0
    } else {
        g.measure(ampm, small, 600) + 2.0 * k
    };
    let x0 = x + (w - wb - ws - wa) / 2.0;
    let (ty, th) = (y + 4.0 * k, 24.0 * k);
    g.text(&hm, x0, ty, wb + 1.0, th, big, 600, v.th.text, Align::Left);
    // seconds and AM/PM sit lower, near the baseline of the big digits
    g.text(
        &secs,
        x0 + wb,
        ty + 3.0 * k,
        ws + 1.0,
        th,
        small,
        500,
        v.th.sub,
        Align::Left,
    );
    g.text(
        ampm,
        x0 + wb + ws + 2.0 * k,
        ty + 3.0 * k,
        wa,
        th,
        small,
        600,
        v.th.sub,
        Align::Left,
    );
    let date = format!(
        "{} {} {}",
        crate::dates::day_name(crate::dates::weekday(t.days())),
        t.day,
        crate::dates::month_abbr(t.month)
    );
    g.text(
        &date,
        x,
        y + h - 18.0 * k,
        w,
        15.0 * k,
        font(k, 10.0),
        500,
        v.th.sub,
        Align::Center,
    );
    hits.push((x, y, w, h, act));
}

/// Just the time, hours and minutes.
fn small_clock(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let act = Action::Open(Panel::Calendar);
    tile(g, v, x, y, w, h, v.hover == Some(act));
    let t = d.time;
    let time = if v.s.clock24 {
        format!("{}:{:02}", t.hour, t.minute)
    } else {
        let ampm = if t.hour < 12 { "AM" } else { "PM" };
        format!("{}:{:02} {ampm}", crate::dates::hour12(t.hour), t.minute)
    };
    let f = font(v.m.k, 17.0);
    g.text(&time, x, y, w, h, f, 600, v.th.text, Align::Center);
    hits.push((x, y, w, h, act));
}

/// The red tear-off clock from the first version.
fn dateclock(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    let t = d.time;
    let r = v.m.r_tile;
    let split = h * 0.5;
    g.fill_rr(x, y, w, h, r, rgba(0x111111, 1.0));
    g.clip(x, y, w, split);
    g.fill_rr(x, y, w, h, r, rgba(0xEE3800, 1.0));
    g.unclip();
    g.stroke_rr(x, y, w, h, r, v.th.tile_border, 1.0);
    // no AM/PM here: the tile is too narrow
    let time = if v.s.clock24 {
        format!("{:02}:{:02}", t.hour, t.minute)
    } else {
        format!("{}:{:02}", crate::dates::hour12(t.hour), t.minute)
    };
    g.text(
        &time,
        x,
        y + 1.0 * k,
        w,
        split,
        font(k, 15.0),
        500,
        rgba(0x2A0E02, 0.92),
        Align::Center,
    );
    // weekday and day on one line: at 10 DIPs there's no room to stack them
    let (by, bh) = (y + split, h - split);
    let wd = crate::dates::day_name(crate::dates::weekday(t.days()));
    let day = t.day.to_string();
    let (fw, fd) = (font(k, 10.0), font(k, 12.0));
    let (ww, dw, sp) = (g.measure(wd, fw, 500), g.measure(&day, fd, 600), 3.0 * k);
    let x0 = x + (w - ww - sp - dw) / 2.0;
    g.text(wd, x0, by, ww + 1.0, bh, fw, 500, white(0.75), Align::Left);
    g.text(
        &day,
        x0 + ww + sp,
        by,
        dw + 1.0,
        bh,
        fd,
        600,
        white(1.0),
        Align::Left,
    );
    hits.push((x, y, w, h, Action::Open(Panel::Calendar)));
}

pub fn limit_color(pct: f32) -> C {
    rgba(
        if pct >= 90.0 {
            0xFF453A
        } else if pct >= 70.0 {
            0xFF9F0A
        } else {
            0x30D158
        },
        1.0,
    )
}

/// The panel an AI limits tile opens.
fn ai_panel(w: Widget) -> Panel {
    match w {
        Widget::Codex => Panel::Codex,
        Widget::Antigravity => Panel::Antigravity,
        Widget::Cursor => Panel::Cursor,
        _ => Panel::Claude,
    }
}

/// Claude: an orange square with a starburst. Codex: a white square with a hexagon.
/// Antigravity: a white square with Gemini's four-pointed star. Cursor: a cube on black.
pub fn ai_logo(g: &Gfx, which: Panel, x: f32, y: f32, s: f32) {
    let (cx, cy) = (x + s / 2.0, y + s / 2.0);
    if which == Panel::Cursor {
        g.fill_rr(x, y, s, s, s * 0.24, rgba(0x141414, 1.0));
        let p = |i: usize| {
            let a = i as f32 * PI / 3.0;
            (cx + a.sin() * s * 0.28, cy - a.cos() * s * 0.28)
        };
        for i in 0..6 {
            let ((x0, y0), (x1, y1)) = (p(i), p(i + 1));
            g.line(x0, y0, x1, y1, white(1.0), s * 0.06);
        }
        for i in [0, 2, 4] {
            let (x1, y1) = p(i);
            g.line(cx, cy, x1, y1, white(0.7), s * 0.05);
        }
    } else if which == Panel::Antigravity {
        g.fill_rr(x, y, s, s, s * 0.24, white(1.0));
        let (r, n) = (s * 0.34, s * 0.06); // tip and waist
        let star: Vec<(f32, f32)> = (0..8)
            .map(|i| {
                let a = i as f32 * PI / 4.0;
                let d = if i % 2 == 0 { r } else { n };
                (cx + a.sin() * d, cy - a.cos() * d)
            })
            .collect();
        g.poly(&star, rgba(0x4C7BF3, 1.0));
    } else if which == Panel::Claude {
        g.fill_rr(x, y, s, s, s * 0.24, rgba(0xD97757, 1.0));
        for i in 0..8 {
            let a = i as f32 * PI / 4.0 + 0.2;
            g.line(
                cx + a.sin() * s * 0.07,
                cy - a.cos() * s * 0.07,
                cx + a.sin() * s * 0.31,
                cy - a.cos() * s * 0.31,
                white(1.0),
                s * 0.085,
            );
        }
    } else {
        g.fill_rr(x, y, s, s, s * 0.24, white(1.0));
        let p = |i: usize| {
            let a = i as f32 * PI / 3.0;
            (cx + a.sin() * s * 0.27, cy - a.cos() * s * 0.27)
        };
        for i in 0..6 {
            let ((x0, y0), (x1, y1)) = (p(i), p(i + 1));
            g.line(x0, y0, x1, y1, rgba(0x111111, 1.0), s * 0.08);
        }
    }
}

/// The medium and small AI tiles: a short label and the share left of each of the first two
/// limits, with the time to reset between them on the medium one.
fn ai_compact(
    g: &Gfx,
    v: &View,
    d: &Data,
    widget: Widget,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let (k, which) = (v.m.k, ai_panel(widget));
    let medium = v.s.variant(widget) == Variant::Medium;
    let act = Action::Open(which);
    tile(g, v, x, y, w, h, v.hover == Some(act));
    ai_logo(g, which, x + 6.0 * k, y + (h - 30.0 * k) / 2.0, 30.0 * k);
    let f = font(k, 10.0);
    let (lx, right) = (x + 42.0 * k, x + w - 7.0 * k);
    let pct_w = g.measure("100%", f, 600);
    for (row, l) in d.limits(which).unwrap_or(&[]).iter().take(2).enumerate() {
        let (ry, rh) = (y + 6.0 * k + row as f32 * 18.0 * k, 16.0 * k);
        let label = if medium {
            l.window.abbr()
        } else {
            l.window.letter()
        };
        g.text(label, lx, ry, 28.0 * k, rh, f, 700, v.th.text, Align::Left);
        let left = format!("{:.0}%", (100.0 - l.pct).clamp(0.0, 100.0));
        g.text(
            &left,
            right - pct_w,
            ry,
            pct_w,
            rh,
            f,
            600,
            limit_text(v, l.pct),
            Align::Right,
        );
        if let (true, Some(r)) = (medium, l.resets) {
            let t = crate::dates::fmt_left(r - d.now);
            let tx = lx + 26.0 * k;
            g.text(
                &t,
                tx,
                ry,
                right - pct_w - 4.0 * k - tx,
                rh,
                f,
                500,
                v.th.sub,
                Align::Left,
            );
        }
    }
    hits.push((x, y, w, h, act));
}

/// The share left, in the bar's text colour until it runs low.
fn limit_text(v: &View, used: f32) -> C {
    if used >= 90.0 {
        rgba(0xFF453A, 1.0)
    } else {
        v.th.text
    }
}

fn ai(
    g: &Gfx,
    v: &View,
    d: &Data,
    which: Panel,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    let act = Action::Open(which);
    tile(g, v, x, y, w, h, v.hover == Some(act));
    ai_logo(g, which, x + 6.0 * k, y + (h - 30.0 * k) / 2.0, 30.0 * k);
    // the first two limits, which every source lists first: session and week, or the month
    let (limits, now) = (d.limits(which).unwrap_or(&[]), d.now);
    let rows: Vec<&Limit> = limits.iter().take(2).collect();
    // columns fit their widest text, so the 10 DIP floor never cuts a label short
    let f = font(k, 10.0);
    let label_w = rows
        .iter()
        .map(|r| g.measure(r.window.short(), f, 600))
        .fold(0.0, f32::max)
        + 4.0 * k;
    let (pct_w, reset_w, sp) = (
        g.measure("100%", f, 600),
        g.measure("23h 59m", f, 500),
        4.0 * k,
    );
    let lx = x + 42.0 * k;
    let pct_x = x + w - 6.0 * k - pct_w;
    let reset_x = pct_x - sp - reset_w;
    let (bx, bw) = (lx + label_w, (reset_x - sp - lx - label_w).max(0.0));
    for (row, l) in rows.into_iter().enumerate() {
        let (ry, rh) = (y + 6.0 * k + row as f32 * 18.0 * k, 16.0 * k);
        g.text(
            l.window.short(),
            lx,
            ry,
            label_w,
            rh,
            f,
            600,
            v.th.text,
            Align::Left,
        );
        let by = ry + rh / 2.0 - 1.5 * k;
        g.fill_rr(bx, by, bw, 3.0 * k, 1.5 * k, v.th.chip);
        g.fill_rr(
            bx,
            by,
            bw * (l.pct / 100.0).clamp(0.0, 1.0),
            3.0 * k,
            1.5 * k,
            limit_color(l.pct),
        );
        if let Some(r) = l.resets {
            g.text(
                &crate::dates::fmt_left(r - now),
                reset_x,
                ry,
                reset_w + 1.0,
                rh,
                f,
                500,
                v.th.sub,
                Align::Left,
            );
        }
        g.text(
            &format!("{:.0}%", l.pct),
            pct_x,
            ry,
            pct_w + 1.0,
            rh,
            f,
            600,
            v.th.text,
            Align::Right,
        );
    }
    hits.push((x, y, w, h, act));
}

impl Data {
    /// What an AI limits panel shows. None until its source has answered.
    pub fn limits(&self, p: Panel) -> Option<&[Limit]> {
        match p {
            Panel::Claude => self.claude.as_deref(),
            Panel::Codex => self.codex.as_deref(),
            Panel::Antigravity => self.antigravity.as_deref(),
            Panel::Cursor => self.cursor.as_deref(),
            _ => None,
        }
    }

    /// Whether the radio, VPN, airplane mode or hotspot behind `act` is on. None when it
    /// can't be told.
    pub fn radio(&self, act: Action) -> Option<bool> {
        match act {
            Action::Wifi => self.wifi,
            Action::Bluetooth => self.bluetooth,
            Action::Vpn => self.vpn,
            Action::Airplane => self.airplane,
            Action::Hotspot => self.hotspot,
            _ => None,
        }
    }
}

/// The Wi-Fi, Bluetooth, VPN, airplane or hotspot glyph for `act`, sized by `s`.
pub fn radio_glyph(g: &Gfx, act: Action, cx: f32, cy: f32, s: f32, c: C) {
    match act {
        Action::Wifi => wifi_glyph(g, cx, cy - s * 0.17, s, c),
        Action::Bluetooth => bluetooth_glyph(g, cx, cy, s * 1.22, c),
        Action::Airplane => plane_glyph(g, cx, cy, s * 1.5, c),
        Action::Hotspot => hotspot_glyph(g, cx, cy, s * 1.4, c),
        _ => shield_glyph(g, cx, cy, s * 1.44, c),
    }
}

/// The bar action of a toggle button.
fn radio_action(c: Control) -> Action {
    match c {
        Control::Wifi => Action::Wifi,
        Control::Bluetooth => Action::Bluetooth,
        Control::Airplane => Action::Airplane,
        Control::Hotspot => Action::Hotspot,
        _ => Action::Vpn,
    }
}

/// A plane seen from above, nose up.
fn plane_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, c: C) {
    let half = [
        (0.0, -0.5),
        (0.06, -0.44),
        (0.06, -0.12),
        (0.48, 0.1),
        (0.48, 0.18),
        (0.06, 0.06),
        (0.06, 0.3),
        (0.19, 0.4),
        (0.19, 0.47),
        (0.0, 0.42),
    ];
    // the right half, then the same mirrored back up the left
    let pts: Vec<(f32, f32)> = half
        .iter()
        .chain(
            half.iter()
                .rev()
                .skip(1)
                .map(|&(x, y)| (-x, y))
                .collect::<Vec<_>>()
                .iter(),
        )
        .map(|&(x, y)| (cx + x * s, cy + y * s))
        .collect();
    g.poly(&pts, c);
}

/// A dot with waves going out to both sides.
fn hotspot_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, c: C) {
    g.fill_circle(cx, cy, s * 0.09, c);
    for r in [0.24, 0.42] {
        g.arc_between(cx, cy, s * r, 0.8, 2.34, c, s * 0.08);
        g.arc_between(cx, cy, s * r, -2.34, -0.8, c, s * 0.08);
    }
}

/// The glyph of a quick-controls button, sized like `radio_glyph`.
pub fn control_glyph(g: &Gfx, c: Control, cx: f32, cy: f32, s: f32, col: C) {
    match c {
        Control::Wifi
        | Control::Bluetooth
        | Control::Vpn
        | Control::Airplane
        | Control::Hotspot => radio_glyph(g, radio_action(c), cx, cy, s, col),
        Control::Volume => speaker_glyph(g, cx, cy, s * 1.5, false, col),
        Control::Battery => battery_glyph(g, cx, cy, s * 1.5, col),
    }
}

pub fn speaker_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, muted: bool, c: C) {
    g.fill_rr(cx - s * 0.5, cy - s * 0.16, s * 0.22, s * 0.32, s * 0.04, c);
    g.poly(
        &[
            (cx - s * 0.32, cy - s * 0.16),
            (cx - s * 0.02, cy - s * 0.42),
            (cx - s * 0.02, cy + s * 0.42),
            (cx - s * 0.32, cy + s * 0.16),
        ],
        c,
    );
    if muted {
        g.line(
            cx + s * 0.14,
            cy - s * 0.16,
            cx + s * 0.46,
            cy + s * 0.16,
            c,
            s * 0.09,
        );
        g.line(
            cx + s * 0.46,
            cy - s * 0.16,
            cx + s * 0.14,
            cy + s * 0.16,
            c,
            s * 0.09,
        );
    } else {
        g.arc_between(cx - s * 0.02, cy, s * 0.26, 0.75, 2.39, c, s * 0.09);
        g.arc_between(cx - s * 0.02, cy, s * 0.46, 0.75, 2.39, c, s * 0.09);
    }
}

fn battery_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, c: C) {
    let (w, h, t) = (s * 0.78, s * 0.44, s * 0.07);
    let (x, y) = (cx - w / 2.0 - s * 0.04, cy - h / 2.0);
    g.stroke_rr(x, y, w, h, s * 0.1, c, t);
    g.fill_rr(x + w + t, cy - h * 0.2, s * 0.06, h * 0.4, s * 0.03, c);
    g.fill_rr(
        x + 2.5 * t,
        y + 2.5 * t,
        (w - 5.0 * t) * 0.7,
        h - 5.0 * t,
        s * 0.03,
        c,
    );
}

fn wifi_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, c: C) {
    let by = cy + s * 0.42;
    g.fill_circle(cx, by, s * 0.11, c);
    g.arc_between(cx, by, s * 0.45, -0.75, 0.75, c, s * 0.15);
    g.arc_between(cx, by, s * 0.8, -0.75, 0.75, c, s * 0.15);
}

fn bluetooth_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, c: C) {
    let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let pts = [
        p(-0.28, -0.24),
        p(0.28, 0.24),
        p(0.0, 0.5),
        p(0.0, -0.5),
        p(0.28, -0.24),
        p(-0.28, 0.24),
    ];
    for w in pts.windows(2) {
        g.line(w[0].0, w[0].1, w[1].0, w[1].1, c, s * 0.12);
    }
}

/// Small utility tiles (hidden icons, show desktop) only show a backing under the cursor.
fn hover_backing(g: &Gfx, v: &View, act: Action, x: f32, y: f32, w: f32, h: f32) {
    if v.hover == Some(act) {
        g.fill_rr(x, y, w, h, v.m.r_tile, v.th.tile_hover);
    }
}

/// A chevron that opens the hidden-icons panel, pointing the way the panel opens.
fn tray(g: &Gfx, v: &View, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let act = Action::Open(Panel::Tray);
    hover_backing(g, v, act, x, y, w, h);
    let (cx, cy, s) = (x + w / 2.0, y + h / 2.0, 3.8 * v.m.k);
    let d = if v.s.top { 1.0 } else { -1.0 };
    g.line(
        cx - s,
        cy - d * s / 2.0,
        cx,
        cy + d * s / 2.0,
        v.th.text,
        1.8 * v.m.k,
    );
    g.line(
        cx,
        cy + d * s / 2.0,
        cx + s,
        cy - d * s / 2.0,
        v.th.text,
        1.8 * v.m.k,
    );
    hits.push((x, y, w, h, act));
}

/// A small screen with a stand. Clicking it shows the desktop (Win+D).
fn desktop(g: &Gfx, v: &View, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    hover_backing(g, v, Action::ShowDesktop, x, y, w, h);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    g.stroke_rr(
        cx - 6.5 * k,
        cy - 6.0 * k,
        13.0 * k,
        9.0 * k,
        1.8 * k,
        v.th.text,
        1.4 * k,
    );
    g.line(cx, cy + 3.0 * k, cx, cy + 5.5 * k, v.th.text, 1.4 * k);
    g.line(
        cx - 3.5 * k,
        cy + 6.0 * k,
        cx + 3.5 * k,
        cy + 6.0 * k,
        v.th.text,
        1.4 * k,
    );
    hits.push((x, y, w, h, Action::ShowDesktop));
}

fn lang(g: &Gfx, v: &View, d: &Data, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let Some((code, region)) = &d.lang else {
        return;
    };
    let k = v.m.k;
    tile(g, v, x, y, w, h, v.hover == Some(Action::Language));
    g.text(
        code,
        x,
        y + 7.0 * k,
        w,
        18.0 * k,
        font(k, 12.0),
        700,
        v.th.text,
        Align::Center,
    );
    g.text(
        region,
        x,
        y + 24.0 * k,
        w,
        15.0 * k,
        font(k, 10.0),
        500,
        v.th.sub,
        Align::Center,
    );
    hits.push((x, y, w, h, Action::Language));
}

/// Width of one part of the quick controls tile, before the size factor.
fn control_w(c: Control, d: &Data) -> f32 {
    match c {
        Control::Wifi
        | Control::Bluetooth
        | Control::Vpn
        | Control::Airplane
        | Control::Hotspot => 30.0,
        Control::Volume => 100.0,
        Control::Battery if d.battery.is_some() => 38.0,
        Control::Battery => 0.0,
    }
}

fn shield_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, c: C) {
    let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
    g.poly(
        &[
            p(0.0, -0.5),
            p(0.42, -0.32),
            p(0.38, 0.12),
            p(0.0, 0.5),
            p(-0.38, 0.12),
            p(-0.42, -0.32),
        ],
        c,
    );
}

/// The buttons chosen in Settings work in place. Anywhere else opens the quick controls panel.
fn controls(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    tile(
        g,
        v,
        x,
        y,
        w,
        h,
        v.hover == Some(Action::Open(Panel::Controls)),
    );
    let (cy, br) = (y + h / 2.0, 13.0 * k);
    let mut cur = x + 4.0 * k;
    for &part in &v.s.controls {
        let pw = control_w(part, d) * k;
        match part {
            Control::Wifi
            | Control::Bluetooth
            | Control::Vpn
            | Control::Airplane
            | Control::Hotspot => {
                let act = radio_action(part);
                let on = d.radio(act);
                let cx = cur + pw / 2.0;
                let lit = on == Some(true);
                let mut bg = if lit { v.accent } else { v.th.chip };
                if v.hover == Some(act) && !lit {
                    bg.a *= 1.8;
                }
                g.fill_circle(cx, cy, br, bg);
                let fg = dim(
                    if lit { on_fill(v.accent) } else { v.th.text },
                    if on.is_some() { 1.0 } else { 0.35 },
                );
                radio_glyph(g, act, cx, cy, 9.0 * k, fg);
                hits.push((cx - br, cy - br, br * 2.0, br * 2.0, act));
            }
            Control::Volume => {
                let (vx, vw) = (cur + 4.0 * k, pw - 8.0 * k);
                let (name, level, muted) = d.volume.as_ref().map_or(("Volume", 0.0, false), |vo| {
                    (vo.label(), vo.level, vo.muted)
                });
                g.text(
                    name,
                    vx,
                    y + 5.0 * k,
                    vw - 42.0 * k,
                    15.0 * k,
                    font(k, 10.0),
                    500,
                    v.th.sub,
                    Align::Left,
                );
                let pct = if muted {
                    "Muted".to_string()
                } else {
                    format!("{:.0}%", level * 100.0)
                };
                g.text(
                    &pct,
                    vx + vw - 40.0 * k,
                    y + 5.0 * k,
                    40.0 * k,
                    15.0 * k,
                    font(k, 10.0),
                    600,
                    v.th.text,
                    Align::Right,
                );
                let by = y + h - 16.0 * k;
                g.fill_rr(vx, by, vw, 5.0 * k, 2.5 * k, v.th.chip);
                g.fill_rr(
                    vx,
                    by,
                    vw * level.clamp(0.0, 1.0),
                    5.0 * k,
                    2.5 * k,
                    if muted { v.th.sub } else { v.accent },
                );
                let hh = (17.0 * k).max(20.0);
                hits.push((vx, by + 2.5 * k - hh / 2.0, vw, hh, Action::Volume));
            }
            Control::Battery => {
                if let Some((f, charging)) = d.battery {
                    let s = 30.0 * k;
                    let bx = cur + 4.0 * k;
                    let bg = if charging {
                        rgba(0x30D158, 1.0)
                    } else {
                        v.accent
                    };
                    g.fill_rr(bx, cy - s / 2.0, s, s, 9.0 * k, bg);
                    g.text(
                        &format!("{:.0}", f * 100.0),
                        bx,
                        cy - s / 2.0,
                        s,
                        s,
                        font(k, 12.0),
                        700,
                        on_fill(bg),
                        Align::Center,
                    );
                }
            }
        }
        cur += pw;
    }
    hits.push((x, y, w, h, Action::Open(Panel::Controls))); // last, so the controls above win
}

/// The Windows 11 Start logo: four blue panes straight on the bar, no tile behind them.
/// Windows ships it neither as a font glyph nor as a public asset, so it's drawn here.
fn start_logo(g: &Gfx, x: f32, y: f32, s: f32) {
    let side = s * 0.66;
    let gap = side * 0.06;
    let p = (side - gap) / 2.0;
    let (lx, ly) = (x + (s - side) / 2.0, y + (s - side) / 2.0);
    for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let (px, py) = (lx + dx * (p + gap), ly + dy * (p + gap));
        g.fill_rr_grad(
            px,
            py,
            p,
            p,
            p * 0.08,
            &[(0.0, rgba(0x5CC8FF, 1.0)), (1.0, rgba(0x0067C0, 1.0))],
            true,
        );
    }
}

/// Dock icons `range` (0 is Start, then the apps). Magnified icons grow away from the screen
/// edge, into the room beside the bar.
fn icons(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    h: f32,
    range: std::ops::Range<usize>,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    let s0 = v.m.icon;
    let iy0 = y + (h - s0) / 2.0 - 1.5 * k;
    let mut ix = x + ICON_LEAD * k;
    let mut lifted = None;
    for j in range {
        let m = v.mag.get(j).copied().unwrap_or(1.0);
        let s = s0 * m;
        let iy = if v.s.top { iy0 } else { iy0 + s0 - s };
        let act = if j == 0 {
            Action::Start
        } else {
            Action::App(j - 1)
        };
        let top = iy.min(y);
        hits.push((
            ix - ICON_GAP * k / 2.0,
            top,
            s + ICON_GAP * k,
            (y + h).max(iy + s) - top,
            act,
        ));
        let dx = if j == 0 {
            0.0
        } else {
            v.slide.get(j - 1).copied().unwrap_or(0.0)
        };
        if j > 0 && v.lifted == Some(j - 1) {
            lifted = Some((j, ix + dx, iy, m));
        } else {
            icon(g, v, d, j, ix + dx, iy, m, y, h);
        }
        ix += s + ICON_GAP * k;
    }
    // the dragged icon goes on top, a little larger, as if picked up
    if let Some((j, ix, iy, m)) = lifted {
        let s = s0 * m;
        let grow = 0.08 * s;
        icon(g, v, d, j, ix - grow / 2.0, iy - grow, m * 1.08, y, h);
    }
}

/// One dock icon (0 is Start) with its backing, running dot and badge.
fn icon(g: &Gfx, v: &View, d: &Data, j: usize, ix: f32, iy: f32, m: f32, y: f32, h: f32) {
    let k = v.m.k;
    let s = v.m.icon * m;
    let act = if j == 0 {
        Action::Start
    } else {
        Action::App(j - 1)
    };
    let lift = ((m - 1.0) / (MAG - 1.0)).clamp(0.0, 1.0);
    let held = j > 0 && v.lifted == Some(j - 1);
    let backing = if v.hover == Some(act) || held {
        lift.max(0.6)
    } else {
        lift * 0.7
    };
    if backing > 0.02 {
        let pad = 3.0 * k * m;
        g.fill_rr(
            ix - pad,
            iy - pad,
            s + 2.0 * pad,
            s + 2.0 * pad,
            10.0 * k * m,
            dim(v.th.tile_hover, backing),
        );
    }
    if j == 0 {
        start_logo(g, ix, iy, s);
        return;
    }
    let a = &d.apps[j - 1];
    match g.cached(a.key()) {
        Some(b) => {
            // lifts the icon off light glass
            g.bitmap_shadow(&b, ix, iy, s, s, 2.0 * k * m, 1.5 * k * m, 0.35);
            g.bitmap(&b, ix, iy, s, s)
        }
        None => g.fill_rr(ix, iy, s, s, 9.0 * k * m, v.th.chip),
    }
    if !a.windows.is_empty() {
        g.fill_circle(ix + s / 2.0, y + h - 1.5 * k, 2.0 * k, v.accent);
    }
    if a.badge > 0 {
        let r = (6.5 * k * m.sqrt()).max(7.5);
        let (bx, by) = (ix + s - 2.0 * k * m, iy + 2.0 * k * m);
        g.fill_circle(bx, by, r, rgba(0xFF3B30, 1.0));
        let n = if a.badge > 99 {
            "99+".to_string()
        } else {
            a.badge.to_string()
        };
        g.text(
            &n,
            bx - r * 1.5,
            by - r,
            r * 3.0,
            r * 2.0,
            font(k * m.sqrt(), 8.0),
            700,
            white(1.0),
            Align::Center,
        );
    }
}

/// A progress ring with its value inside and its label under it, in a tile `h` tall at `y`.
fn ring(g: &Gfx, v: &View, cx: f32, y: f32, h: f32, frac: f32, color: C, label: &str, value: &str) {
    let k = v.m.k;
    let r = 13.0 * k;
    let cy = y + 4.0 * k + r;
    g.arc(cx, cy, r, 1.0, dim(v.th.text, 0.12), 3.2 * k);
    g.arc(cx, cy, r, frac, color, 3.2 * k);
    g.text(
        value,
        cx - r,
        cy - r,
        r * 2.0,
        r * 2.0,
        font(k, 11.0),
        600,
        v.th.text,
        Align::Center,
    );
    let ly = cy + r + 1.5 * k;
    g.text(
        label,
        cx - 28.0 * k,
        ly,
        56.0 * k,
        y + h - ly,
        font(k, 10.0),
        500,
        v.th.sub,
        Align::Center,
    );
}

fn system(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    tile(g, v, x, y, w, h, v.hover == Some(Action::TaskManager));
    for (fx, frac, label) in [(0.28, d.cpu, "CPU"), (0.72, d.mem, "RAM")] {
        let value = format!("{:.0}", frac * 100.0);
        ring(g, v, x + w * fx, y, h, frac, v.accent, label, &value);
    }
    hits.push((x, y, w, h, Action::TaskManager));
}

fn battery(g: &Gfx, v: &View, d: &Data, x: f32, y: f32, w: f32, h: f32) {
    let Some((f, charging)) = d.battery else {
        return;
    };
    tile(g, v, x, y, w, h, false);
    let c = if f < 0.1 {
        0xFF453A
    } else if f < 0.2 && !charging {
        0xFF9F0A
    } else {
        0x30D158
    };
    let label = if charging { "Charging" } else { "Battery" };
    let value = format!("{:.0}", f * 100.0);
    ring(g, v, x + w / 2.0, y, h, f, rgba(c, 1.0), label, &value);
}

/// A microphone: a capsule on a stand, struck through when muted.
pub fn mic_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, muted: bool, c: C) {
    let t = s * 0.09;
    g.stroke_rr(
        cx - s * 0.16,
        cy - s * 0.48,
        s * 0.32,
        s * 0.56,
        s * 0.16,
        c,
        t,
    );
    g.arc_between(
        cx,
        cy - s * 0.12,
        s * 0.3,
        PI / 2.0 + 0.15,
        1.5 * PI - 0.15,
        c,
        t,
    );
    g.line(cx, cy + s * 0.18, cx, cy + s * 0.42, c, t);
    if muted {
        g.line(
            cx - s * 0.36,
            cy - s * 0.42,
            cx + s * 0.36,
            cy + s * 0.34,
            c,
            t,
        );
    }
}

/// Headphones: a band and two ear cups, struck through when the sound is off.
pub fn headphones_glyph(g: &Gfx, cx: f32, cy: f32, s: f32, muted: bool, c: C) {
    let t = s * 0.09;
    g.arc_between(cx, cy + s * 0.04, s * 0.36, -PI / 2.0, PI / 2.0, c, t);
    for dx in [-1.0, 1.0] {
        let x = cx + dx * s * 0.36 - s * 0.1;
        g.fill_rr(x, cy - s * 0.02, s * 0.2, s * 0.38, s * 0.07, c);
    }
    if muted {
        g.line(
            cx - s * 0.42,
            cy - s * 0.4,
            cx + s * 0.42,
            cy + s * 0.38,
            c,
            t,
        );
    }
}

/// Mute the microphone and deafen (mute the speakers), each lit red while it's on.
fn mic(g: &Gfx, v: &View, d: &Data, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    tile(g, v, x, y, w, h, false);
    let (cy, br) = (y + h / 2.0, 13.0 * k);
    let deaf = d.volume.as_ref().map(|vo| vo.muted);
    for (i, (act, on)) in [(Action::Mic, d.mic), (Action::Mute, deaf)]
        .into_iter()
        .enumerate()
    {
        let cx = x + 4.0 * k + (i as f32 + 0.5) * 30.0 * k;
        let lit = on == Some(true);
        let red = rgba(0xFF453A, 1.0);
        let mut bg = if lit { red } else { v.th.chip };
        if v.hover == Some(act) && !lit {
            bg.a *= 1.8;
        }
        g.fill_circle(cx, cy, br, bg);
        let fg = dim(
            if lit { on_fill(red) } else { v.th.text },
            if on.is_some() { 1.0 } else { 0.35 },
        );
        if act == Action::Mic {
            mic_glyph(g, cx, cy, 15.0 * k, lit, fg)
        } else {
            headphones_glyph(g, cx, cy, 15.0 * k, lit, fg)
        }
        hits.push((cx - br, cy - br, br * 2.0, br * 2.0, act));
    }
}

/// Download and upload speed, one per line. Opens Task Manager, which has the graphs.
fn network(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    tile(g, v, x, y, w, h, v.hover == Some(Action::TaskManager));
    let (down, up) = d.net.unwrap_or((0.0, 0.0));
    let f = font(k, 11.0);
    for (i, (arrow, bytes)) in [("↓", down), ("↑", up)].into_iter().enumerate() {
        let ry = y + 5.0 * k + i as f32 * 17.0 * k;
        g.text(
            arrow,
            x + 8.0 * k,
            ry,
            12.0 * k,
            16.0 * k,
            f,
            700,
            v.accent,
            Align::Left,
        );
        let speed = crate::net::fmt_rate(bytes);
        let (tx, tw) = (x + 20.0 * k, w - 28.0 * k);
        g.text(
            &speed,
            tx,
            ry,
            tw,
            16.0 * k,
            f,
            600,
            v.th.text,
            Align::Right,
        );
    }
    hits.push((x, y, w, h, Action::TaskManager));
}

/// A circle, half filled: switches Windows between light and dark mode.
fn theme_toggle(g: &Gfx, v: &View, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    hover_backing(g, v, Action::Theme, x, y, w, h);
    let (cx, cy, r) = (x + w / 2.0, y + h / 2.0, 6.5 * k);
    g.arc(cx, cy, r, 1.0, v.th.text, 1.4 * k);
    let half: Vec<(f32, f32)> = (0..=16)
        .map(|i| {
            let a = PI + i as f32 * PI / 16.0; // the left half, bottom to top
            (cx + r * a.sin(), cy - r * a.cos())
        })
        .collect();
    g.poly(&half, v.th.text);
    hits.push((x, y, w, h, Action::Theme));
}

/// How far through today, this month and this year we are, as three rings.
fn progress(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let act = Action::Open(Panel::Calendar);
    tile(g, v, x, y, w, h, v.hover == Some(act));
    let fracs = crate::dates::progress(d.time);
    for (i, (frac, label)) in fracs.into_iter().zip(["Day", "Month", "Year"]).enumerate() {
        let value = format!("{:.0}", (frac * 100.0).floor());
        let cx = x + w * (i as f32 * 2.0 + 1.0) / 6.0;
        ring(g, v, cx, y, h, frac, v.accent, label, &value);
    }
    hits.push((x, y, w, h, act));
}

/// DeepSeek's mark, roughly: a white wave on its blue.
fn deepseek_logo(g: &Gfx, x: f32, y: f32, s: f32) {
    g.fill_rr(x, y, s, s, s * 0.24, rgba(0x4D6BFE, 1.0));
    let (cx, cy) = (x + s / 2.0, y + s * 0.52);
    g.arc_between(cx, cy, s * 0.26, 0.9, 2.0 * PI - 0.5, white(1.0), s * 0.09);
    g.fill_circle(cx + s * 0.08, cy - s * 0.08, s * 0.05, white(1.0));
}

/// What's left on the DeepSeek account. Without a key it asks for one in Settings.
fn deepseek(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    let keyed = !v.s.deepseek_key.is_empty();
    let act = if keyed {
        Action::DeepSeek
    } else {
        Action::Settings
    };
    tile(g, v, x, y, w, h, v.hover == Some(act));
    deepseek_logo(g, x + 6.0 * k, y + (h - 30.0 * k) / 2.0, 30.0 * k);
    let (tx, tw) = (x + 42.0 * k, w - 48.0 * k);
    let (label, amount) = match (&d.deepseek, keyed) {
        (Some(b), _) => ("Balance", b.first().map(|b| b.text()).unwrap_or_default()),
        (None, true) => ("Balance", "…".to_string()),
        (None, false) => ("DeepSeek", "Add a key".to_string()),
    };
    g.text(
        label,
        tx,
        y + 5.0 * k,
        tw,
        15.0 * k,
        font(k, 10.0),
        500,
        v.th.sub,
        Align::Left,
    );
    g.text(
        &amount,
        tx,
        y + 19.0 * k,
        tw,
        20.0 * k,
        font(k, 14.0),
        600,
        v.th.text,
        Align::Left,
    );
    hits.push((x, y, w, h, act));
}

/// An envelope with Gmail's red M across its flap.
fn gmail_logo(g: &Gfx, x: f32, y: f32, s: f32) {
    g.fill_rr(x, y, s, s, s * 0.24, white(1.0));
    let (l, r, t, b) = (x + s * 0.2, x + s * 0.8, y + s * 0.3, y + s * 0.72);
    let red = rgba(0xEA4335, 1.0);
    let w = s * 0.09;
    g.line(l, b, l, t, red, w);
    g.line(l, t, x + s / 2.0, y + s * 0.52, red, w);
    g.line(x + s / 2.0, y + s * 0.52, r, t, red, w);
    g.line(r, t, r, b, red, w);
}

/// Unread conversations in the Gmail inbox. Opens Gmail, or Settings to sign in.
fn gmail(g: &Gfx, v: &View, d: &Data, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    let act = match d.gmail {
        Some(Ok(_)) => Action::Gmail,
        _ => Action::Settings,
    };
    tile(g, v, x, y, w, h, v.hover == Some(act));
    gmail_logo(g, x + 6.0 * k, y + (h - 30.0 * k) / 2.0, 30.0 * k);
    let (tx, tw) = (x + 42.0 * k, w - 48.0 * k);
    let (label, value) = match d.gmail {
        Some(Ok(n)) => ("Unread", n.to_string()),
        Some(Err(note)) => ("Gmail", note.to_string()),
        None if d.google => ("Unread", "…".to_string()),
        None => ("Gmail", "Sign in".to_string()),
    };
    let big = if matches!(d.gmail, Some(Ok(_))) {
        16.0
    } else {
        11.0
    };
    g.text(
        label,
        tx,
        y + 5.0 * k,
        tw,
        15.0 * k,
        font(k, 10.0),
        500,
        v.th.sub,
        Align::Left,
    );
    g.text(
        &value,
        tx,
        y + 19.0 * k,
        tw,
        20.0 * k,
        font(k, big),
        600,
        v.th.text,
        Align::Left,
    );
    hits.push((x, y, w, h, act));
}

/// A bell. Clicking it opens the notification centre (Win+N).
fn notifications(g: &Gfx, v: &View, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    let act = Action::Notifications;
    hover_backing(g, v, act, x, y, w, h);
    let (cx, cy, c, t) = (x + w / 2.0, y + h / 2.0, v.th.text, 1.4 * k);
    let (r, top, rim) = (4.2 * k, cy - 2.0 * k, cy + 3.5 * k);
    g.arc_between(cx, top, r, -PI / 2.0, PI / 2.0, c, t);
    g.line(cx - r, top, cx - r - 1.2 * k, rim, c, t);
    g.line(cx + r, top, cx + r + 1.2 * k, rim, c, t);
    g.line(cx - r - 1.8 * k, rim, cx + r + 1.8 * k, rim, c, t);
    g.fill_circle(cx, rim + 2.2 * k, 1.3 * k, c);
    hits.push((x, y, w, h, act));
}

/// Today's date: the weekday over the day and month, or a calendar page with the day number.
fn date(g: &Gfx, v: &View, d: &Data, x: f32, y: f32, w: f32, h: f32, hits: &mut Vec<Hit<Action>>) {
    let k = v.m.k;
    let act = Action::Open(Panel::Calendar);
    tile(g, v, x, y, w, h, v.hover == Some(act));
    let t = d.time;
    let wd = crate::dates::weekday(t.days());
    if v.s.variant(Widget::Date) == Variant::DayNumber {
        let red = rgba(if v.th.dark { 0xFF453A } else { 0xD70015 }, 1.0);
        let short = crate::dates::day_name(wd).to_uppercase();
        g.text(
            &short,
            x,
            y + 4.0 * k,
            w,
            13.0 * k,
            font(k, 10.0),
            700,
            red,
            Align::Center,
        );
        let day = t.day.to_string();
        g.text(
            &day,
            x,
            y + 15.0 * k,
            w,
            h - 17.0 * k,
            font(k, 19.0),
            600,
            v.th.text,
            Align::Center,
        );
    } else {
        let (tx, tw) = (x + 10.0 * k, w - 20.0 * k);
        let name = crate::dates::day_full_name(wd);
        g.text(
            name,
            tx,
            y + 5.0 * k,
            tw,
            15.0 * k,
            font(k, 10.0),
            600,
            v.th.sub,
            Align::Left,
        );
        let day = format!("{} {}", crate::dates::month_name(t.month), t.day);
        g.text(
            &day,
            tx,
            y + 19.0 * k,
            tw,
            20.0 * k,
            font(k, 15.0),
            700,
            v.th.text,
            Align::Left,
        );
    }
    hits.push((x, y, w, h, act));
}

/// How far through the track it is, 0 to 1. 0 when the player gives no length.
fn track_progress(d: &Data) -> f32 {
    d.media
        .as_ref()
        .filter(|m| m.length > 0.0)
        .map_or(0.0, |m| (m.position / m.length) as f32)
        .clamp(0.0, 1.0)
}

/// The cover, rounded, or a plain square with a note on it when the track has none.
fn cover(g: &Gfx, v: &View, x: f32, y: f32, s: f32) {
    let r = s * 0.22;
    match g.cached("art") {
        Some(b) => g.bitmap_rr(&b, x, y, s, s, r),
        None => {
            g.fill_rr(x, y, s, s, r, v.th.chip);
            let (cx, cy) = (x + s * 0.44, y + s * 0.64);
            g.fill_circle(cx, cy, s * 0.1, v.th.sub);
            g.line(
                cx + s * 0.09,
                cy,
                cx + s * 0.09,
                y + s * 0.28,
                v.th.sub,
                s * 0.05,
            );
        }
    }
}

/// A round play or pause button.
fn play_button(g: &Gfx, v: &View, d: &Data, cx: f32, cy: f32, r: f32, hits: &mut Vec<Hit<Action>>) {
    let playing = d.media.as_ref().is_some_and(|m| m.playing);
    let mut bg = v.th.chip;
    if v.hover == Some(Action::Toggle) {
        bg.a *= 1.8;
    }
    g.fill_circle(cx, cy, r, bg);
    let fade = if d.media.is_some() { 1.0 } else { 0.45 };
    media_glyph(
        g,
        Action::Toggle,
        playing,
        cx,
        cy,
        r * 0.34,
        dim(v.th.text, fade),
    );
    hits.push((cx - r, cy - r, 2.0 * r, 2.0 * r, Action::Toggle));
}

/// Extra large: cover, title and artist over a progress bar, play and skip. Medium: cover,
/// title and artist, and play.
fn media_card(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    let xl = v.s.variant(Widget::Media) == Variant::ExtraLarge;
    tile(g, v, x, y, w, h, false);
    let s = h - 12.0 * k;
    cover(g, v, x + 6.0 * k, y + 6.0 * k, s);
    let (title, artist) = match &d.media {
        Some(m) => (m.title.as_str(), m.artist.as_str()),
        None => ("Nothing playing", ""),
    };
    let buttons = if xl { 64.0 } else { 38.0 } * k;
    let (tx, tw) = (x + s + 14.0 * k, w - s - 14.0 * k - buttons);
    let ty = if xl { y + 4.0 * k } else { y + 7.0 * k };
    g.text(
        title,
        tx,
        ty,
        tw,
        15.0 * k,
        font(k, 11.0),
        700,
        v.th.text,
        Align::Left,
    );
    g.text(
        artist,
        tx,
        ty + 14.0 * k,
        tw,
        14.0 * k,
        font(k, 10.0),
        500,
        v.th.sub,
        Align::Left,
    );
    if xl {
        let by = y + h - 10.0 * k;
        g.fill_rr(tx, by, tw, 3.0 * k, 1.5 * k, v.th.chip);
        g.fill_rr(tx, by, tw * track_progress(d), 3.0 * k, 1.5 * k, v.th.text);
    }
    let cy = y + h / 2.0;
    let play_x = x + w - if xl { 46.0 } else { 22.0 } * k;
    play_button(g, v, d, play_x, cy, 13.0 * k, hits);
    if xl {
        let nx = x + w - 16.0 * k;
        if v.hover == Some(Action::Next) {
            g.fill_circle(nx, cy, 11.0 * k, v.th.tile_hover);
        }
        let fade = if d.media.is_some() { 1.0 } else { 0.45 };
        media_glyph(
            g,
            Action::Next,
            false,
            nx,
            cy,
            4.2 * k,
            dim(v.th.text, fade),
        );
        hits.push((nx - 12.0 * k, y, 24.0 * k, h, Action::Next));
    }
}

/// Small: one round play button, with the track's progress running round it.
fn media_mini(
    g: &Gfx,
    v: &View,
    d: &Data,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit<Action>>,
) {
    let k = v.m.k;
    tile(g, v, x, y, w, h, false);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let r = 16.0 * k;
    g.arc(cx, cy, r, 1.0, v.th.chip, 2.6 * k);
    g.arc(cx, cy, r, track_progress(d), v.th.text, 2.6 * k);
    play_button(g, v, d, cx, cy, 12.0 * k, hits);
}
