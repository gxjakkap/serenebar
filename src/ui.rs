//! All units are DIPs.
use crate::dates::LocalTime;
use crate::dock::MAG;
use crate::gfx::{rgba, Align, Gfx, C};
use crate::limits::{Limit, Window};
use crate::settings::{Control, Settings, Size, Slot, Style, Widget, ACCENTS, GROUPS};
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
        sub: rgba(sub, 0.62),
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Panel {
    Weather,
    Calendar,
    Claude,
    Codex,
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
    Mute,
    Volume,
    Settings,
    Language,
    ShowDesktop,
    QuickSettings,
    SystemSettings,
    SettingsFolder,
}

#[derive(Default)]
pub struct Data {
    pub cpu: f32,
    pub mem: f32,
    pub battery: Option<(f32, bool)>,
    pub apps: Vec<crate::apps::AppItem>,
    pub media: Option<crate::controls::Media>,
    pub art_tint: Option<C>,
    pub weather: Option<Weather>,
    pub events: Vec<crate::calendar::Event>,
    pub claude: Option<Vec<Limit>>,
    pub codex: Option<Vec<Limit>>,
    pub volume: Option<crate::controls::Volume>,
    pub wifi: Option<bool>,
    pub vpn: Option<bool>,
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
}

pub fn accent(s: &Settings) -> C {
    rgba(ACCENTS[s.accent].0, 1.0)
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

/// The ✕ in a card's top right corner.
pub fn close_button<A>(g: &Gfx, p: &Pal, cx: f32, cy: f32, act: A, hits: &mut Vec<Hit<A>>) {
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
            sub: rgba(0x3C3C43, 0.6),
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
        Widget::Media => 196.0 * k,
        Widget::Weather => 150.0 * k,
        Widget::Clock => 100.0 * k,
        Widget::DateClock => 52.0 * k,
        Widget::Claude if d.claude.is_some() => 196.0 * k,
        Widget::Codex if d.codex.is_some() => 196.0 * k,
        Widget::Controls => {
            let w: f32 = v.s.controls.iter().map(|&c| control_w(c, d)).sum();
            if w == 0.0 {
                return None;
            }
            (w + 8.0) * k
        }
        Widget::Lang if d.lang.is_some() => 46.0 * k,
        Widget::Tray | Widget::Desktop => 24.0 * k,
        Widget::System => 96.0 * k,
        Widget::Battery if d.battery.is_some() => 48.0 * k,
        Widget::Apps | Widget::Claude | Widget::Codex | Widget::Lang | Widget::Battery => {
            return None
        }
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
        Some(w) => [
            v.m.pad,
            (w - span(&groups[1])) / 2.0,
            w - v.m.pad - span(&groups[2]),
        ],
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
                Widget::Media => media(g, v, d, x, y, w, h, &mut hits),
                Widget::Weather => weather(g, v, d, x, y, w, h, &mut hits),
                Widget::Clock => clock(g, v, d, x, y, w, h, &mut hits),
                Widget::DateClock => dateclock(g, v, d, x, y, w, h, &mut hits),
                Widget::Claude => ai(
                    g,
                    v,
                    Panel::Claude,
                    d.claude.as_deref().unwrap_or(&[]),
                    d.now,
                    x,
                    y,
                    w,
                    h,
                    &mut hits,
                ),
                Widget::Codex => ai(
                    g,
                    v,
                    Panel::Codex,
                    d.codex.as_deref().unwrap_or(&[]),
                    d.now,
                    x,
                    y,
                    w,
                    h,
                    &mut hits,
                ),
                Widget::Controls => controls(g, v, d, x, y, w, h, &mut hits),
                Widget::Lang => lang(g, v, d, x, y, w, h, &mut hits),
                Widget::Tray => tray(g, v, x, y, w, h, &mut hits),
                Widget::Desktop => desktop(g, v, x, y, w, h, &mut hits),
                Widget::System => system(g, v, d, x, y, w, h, &mut hits),
                Widget::Battery => battery(g, v, d, x, y, w, h),
            }
        }
    }
    g.set_transform(Matrix3x2::identity());
    hits
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
        None => ("Not playing".to_string(), false),
    };
    g.text(
        &line,
        x + 10.0 * k,
        y + 5.0 * k,
        w - 20.0 * k,
        15.0 * k,
        10.5 * k,
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
            "Set a city for weather"
        };
        g.text(
            msg,
            x + 8.0 * k,
            y,
            w - 16.0 * k,
            h,
            9.5 * k,
            500,
            v.th.sub,
            Align::Center,
        );
        hits.push((x, y, w, h, act));
        return;
    };
    let (top, bottom) = if wx.day {
        (0x5AA9F0, 0x2F72D6)
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
    g.text(
        label,
        x + 10.0 * k,
        y + 7.0 * k,
        w * 0.56,
        15.0 * k,
        10.5 * k,
        600,
        white(1.0),
        Align::Left,
    );
    g.text(
        &wx.city,
        x + 10.0 * k,
        y + 22.0 * k,
        w * 0.56,
        13.0 * k,
        9.0 * k,
        500,
        white(0.8),
        Align::Left,
    );
    g.text(
        &format!("{:.0}°", wx.temp),
        x + w * 0.5,
        y,
        w * 0.5 - 26.0 * k,
        h,
        21.0 * k,
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
    let (big, small) = (17.0 * k, 8.5 * k);
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
        13.0 * k,
        9.0 * k,
        500,
        v.th.sub,
        Align::Center,
    );
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
        15.0 * k,
        500,
        rgba(0x2A0E02, 0.92),
        Align::Center,
    );
    let (by, bh) = (y + split, h - split);
    g.text(
        crate::dates::day_name(crate::dates::weekday(t.days())),
        x + 5.0 * k,
        by + 1.0 * k,
        w * 0.5,
        bh * 0.38,
        7.0 * k,
        500,
        white(0.75),
        Align::Left,
    );
    g.text(
        &t.day.to_string(),
        x + 5.0 * k,
        by + bh * 0.32,
        w * 0.5,
        bh * 0.62,
        12.0 * k,
        600,
        white(1.0),
        Align::Left,
    );
    let cr = bh / 2.0 - 2.5 * k;
    let (cx, cy) = (x + w - cr - 4.0 * k, by + bh / 2.0);
    g.fill_circle(cx, cy, cr, rgba(0xC9C9CE, 1.0));
    let badge = match &d.weather {
        Some(wx) => format!("{:.0}°", wx.temp),
        None => crate::dates::month_abbr(t.month).to_string(),
    };
    g.text(
        &badge,
        cx - cr,
        cy - cr,
        cr * 2.0,
        cr * 2.0,
        7.5 * k,
        500,
        rgba(0x222222, 1.0),
        Align::Center,
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

/// Claude: an orange square with a starburst. Codex: a white square with a hexagon.
pub fn ai_logo(g: &Gfx, which: Panel, x: f32, y: f32, s: f32) {
    let (cx, cy) = (x + s / 2.0, y + s / 2.0);
    if which == Panel::Claude {
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

fn ai(
    g: &Gfx,
    v: &View,
    which: Panel,
    limits: &[Limit],
    now: i64,
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
    let lx = x + 42.0 * k;
    for (row, window) in [Window::Session, Window::Weekly].into_iter().enumerate() {
        let Some(l) = limits.iter().find(|l| l.window == window) else {
            continue;
        };
        let ry = y + 7.0 * k + row as f32 * 17.0 * k;
        g.text(
            window.label(),
            lx,
            ry,
            40.0 * k,
            12.0 * k,
            8.5 * k,
            600,
            v.th.text,
            Align::Left,
        );
        let (bx, bw) = (lx + 40.0 * k, w - (lx - x) - 40.0 * k - 66.0 * k);
        g.fill_rr(bx, ry + 4.5 * k, bw, 3.0 * k, 1.5 * k, v.th.chip);
        g.fill_rr(
            bx,
            ry + 4.5 * k,
            bw * (l.pct / 100.0).clamp(0.0, 1.0),
            3.0 * k,
            1.5 * k,
            limit_color(l.pct),
        );
        if let Some(r) = l.resets {
            g.text(
                &crate::dates::fmt_left(r - now),
                bx + bw + 3.0 * k,
                ry,
                36.0 * k,
                12.0 * k,
                7.5 * k,
                500,
                v.th.sub,
                Align::Left,
            );
        }
        g.text(
            &format!("{:.0}%", l.pct),
            x + w - 30.0 * k,
            ry,
            24.0 * k,
            12.0 * k,
            8.5 * k,
            600,
            v.th.text,
            Align::Left,
        );
    }
    hits.push((x, y, w, h, act));
}

impl Data {
    /// Whether the Wi-Fi, Bluetooth or VPN behind `act` is on. None when it can't be told.
    pub fn radio(&self, act: Action) -> Option<bool> {
        match act {
            Action::Wifi => self.wifi,
            Action::Bluetooth => self.bluetooth,
            Action::Vpn => self.vpn,
            _ => None,
        }
    }
}

/// The Wi-Fi, Bluetooth or VPN glyph for `act`, sized by `s`.
pub fn radio_glyph(g: &Gfx, act: Action, cx: f32, cy: f32, s: f32, c: C) {
    match act {
        Action::Wifi => wifi_glyph(g, cx, cy - s * 0.17, s, c),
        Action::Bluetooth => bluetooth_glyph(g, cx, cy, s * 1.22, c),
        _ => shield_glyph(g, cx, cy, s * 1.44, c),
    }
}

/// The bar action of a Wi-Fi, Bluetooth or VPN button.
fn radio_action(c: Control) -> Action {
    match c {
        Control::Wifi => Action::Wifi,
        Control::Bluetooth => Action::Bluetooth,
        _ => Action::Vpn,
    }
}

/// The glyph of a quick-controls button, sized like `radio_glyph`.
pub fn control_glyph(g: &Gfx, c: Control, cx: f32, cy: f32, s: f32, col: C) {
    match c {
        Control::Wifi | Control::Bluetooth | Control::Vpn => {
            radio_glyph(g, radio_action(c), cx, cy, s, col)
        }
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
        12.0 * k,
        700,
        v.th.text,
        Align::Center,
    );
    g.text(
        region,
        x,
        y + 24.0 * k,
        w,
        14.0 * k,
        8.5 * k,
        500,
        v.th.sub,
        Align::Center,
    );
    hits.push((x, y, w, h, Action::Language));
}

/// Width of one part of the quick controls tile, before the size factor.
fn control_w(c: Control, d: &Data) -> f32 {
    match c {
        Control::Wifi | Control::Bluetooth | Control::Vpn => 30.0,
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
            Control::Wifi | Control::Bluetooth | Control::Vpn => {
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
                    if lit { white(1.0) } else { v.th.text },
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
                    y + 6.0 * k,
                    vw - 30.0 * k,
                    13.0 * k,
                    8.5 * k,
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
                    vx + vw - 34.0 * k,
                    y + 6.0 * k,
                    34.0 * k,
                    13.0 * k,
                    8.5 * k,
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
                hits.push((vx, by - 6.0 * k, vw, 17.0 * k, Action::Volume));
            }
            Control::Battery => {
                if let Some((f, charging)) = d.battery {
                    let s = 30.0 * k;
                    let bx = cur + 4.0 * k;
                    g.fill_rr(
                        bx,
                        cy - s / 2.0,
                        s,
                        s,
                        9.0 * k,
                        if charging {
                            rgba(0x30D158, 1.0)
                        } else {
                            v.accent
                        },
                    );
                    g.text(
                        &format!("{:.0}", f * 100.0),
                        bx,
                        cy - s / 2.0,
                        s,
                        s,
                        11.0 * k,
                        700,
                        white(1.0),
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
        let r = 6.5 * k * m.sqrt();
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
            8.0 * k * m.sqrt(),
            700,
            white(1.0),
            Align::Center,
        );
    }
}

fn ring(
    g: &Gfx,
    v: &View,
    cx: f32,
    cy: f32,
    r: f32,
    frac: f32,
    color: C,
    label: &str,
    value: &str,
) {
    let k = v.m.k;
    g.arc(cx, cy, r, 1.0, dim(v.th.text, 0.12), 3.2 * k);
    g.arc(cx, cy, r, frac, color, 3.2 * k);
    g.text(
        label,
        cx - r,
        cy - r * 0.62,
        r * 2.0,
        r * 0.5,
        6.5 * k,
        600,
        v.th.sub,
        Align::Center,
    );
    g.text(
        value,
        cx - r,
        cy - r * 0.2,
        r * 2.0,
        r * 0.8,
        11.0 * k,
        600,
        v.th.text,
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
    let r = h / 2.0 - 6.0 * v.m.k;
    let cy = y + h / 2.0;
    ring(
        g,
        v,
        x + w * 0.28,
        cy,
        r,
        d.cpu,
        v.accent,
        "CPU",
        &format!("{:.0}", d.cpu * 100.0),
    );
    ring(
        g,
        v,
        x + w * 0.72,
        cy,
        r,
        d.mem,
        v.accent,
        "RAM",
        &format!("{:.0}", d.mem * 100.0),
    );
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
    let label = if charging { "⚡" } else { "BAT" };
    ring(
        g,
        v,
        x + w / 2.0,
        y + h / 2.0,
        h / 2.0 - 6.0 * v.m.k,
        f,
        rgba(c, 1.0),
        label,
        &format!("{:.0}", f * 100.0),
    );
}
