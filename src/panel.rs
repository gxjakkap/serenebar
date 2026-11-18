//! Popover cards that open above a tile: calendar, weather, AI plan limits. DIPs throughout.
use crate::dates::{
    add_month, civil_from_days, day_name, days_from_civil, fmt_hm, month_abbr, month_name, weekday,
    LocalTime,
};
use crate::gfx::{rgba, Align, Gfx, C};
use crate::limits::Limit;
use crate::settings::Settings;
use crate::ui::{
    accent_text, ai_logo, close_button, limit_color, media_glyph, on_fill, pal, radio_glyph,
    speaker_glyph, wash, Action, Data, Pal, Panel,
};

pub struct State {
    pub kind: Panel,
    pub hover: Option<usize>, // hidden-icons panel: the icon under the cursor
    pub over: Option<Act>,    // the control under the cursor
    pub month: (i32, u32),
    pub selected: i64,   // day shown in the calendar's side list
    pub app_rows: usize, // quick controls: app volume rows the panel was sized for
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    Close,
    PrevMonth,
    NextMonth,
    Today,
    Select(i64),
    TrayIcon(usize),
    Do(Action),
}

pub type Hit = crate::ui::Hit<Act>;

impl State {
    pub fn new(kind: Panel, today: LocalTime) -> Self {
        State {
            kind,
            hover: None,
            over: None,
            month: (today.year, today.month),
            selected: today.days(),
            app_rows: 0,
        }
    }

    /// Handles the acts that only change what the panel shows. False for the rest.
    pub fn on(&mut self, act: Act, today: LocalTime) -> bool {
        match act {
            Act::PrevMonth => self.month = add_month(self.month, -1),
            Act::NextMonth => self.month = add_month(self.month, 1),
            Act::Today => {
                *self = State {
                    hover: self.hover,
                    over: self.over,
                    ..State::new(self.kind, today)
                }
            }
            Act::Select(day) => self.selected = day,
            Act::Close | Act::TrayIcon(_) | Act::Do(_) => return false,
        }
        true
    }
}

fn limits(kind: Panel, d: &Data) -> &[Limit] {
    match kind {
        Panel::Claude => d.claude.as_deref().unwrap_or(&[]),
        _ => d.codex.as_deref().unwrap_or(&[]),
    }
}

/// The volume slider takes presses a little past its ends.
const SLIDER_SLACK: f32 = 10.0;

/// The level the volume slider for `act` would set with the cursor at `x`, even outside it.
pub fn volume_at(hits: &[Hit], act: Action, x: f32) -> Option<f32> {
    let (hx, hw) = crate::ui::hit_span(hits, Act::Do(act))?;
    Some((x - hx - SLIDER_SLACK) / (hw - 2.0 * SLIDER_SLACK))
}

// Rows of the panels that grow with their content. `size` adds them up, the draw functions
// step through them.
const TRAY_COLS: usize = 5;
const TRAY_CELL: f32 = 46.0;
const TRAY_TOP: f32 = 56.0;
const TRAY_TIP: f32 = 48.0;
const AI_TOP: f32 = 72.0;
const AI_ROW: f32 = 56.0;
const AI_SOURCE: f32 = 34.0;
const CONTROLS_ROWS: [f32; 4] = [56.0, 116.0, 164.0, 82.0]; // title, now playing, toggles, volume
const BATTERY_ROW: f32 = 30.0;
const APP_ROW: f32 = 44.0;
/// At most this many apps get a volume slider, so the panel stays on screen.
pub const APP_ROWS: usize = 6;

/// Room the app volume card takes for `rows` apps.
fn apps_h(rows: usize) -> f32 {
    if rows == 0 {
        0.0
    } else {
        rows as f32 * APP_ROW + 20.0
    }
}
const FOOTER: f32 = 48.0;

pub fn size(kind: Panel, d: &Data) -> (f32, f32) {
    match kind {
        Panel::Tray => (
            TRAY_COLS as f32 * TRAY_CELL + 28.0,
            TRAY_TOP + d.tray.len().div_ceil(TRAY_COLS).max(1) as f32 * TRAY_CELL + TRAY_TIP,
        ),
        Panel::Calendar => (660.0, 470.0),
        Panel::Weather => (320.0, 404.0),
        Panel::Claude | Panel::Codex => (
            300.0,
            AI_TOP + limits(kind, d).len().max(1) as f32 * AI_ROW + AI_SOURCE,
        ),
        Panel::About => (360.0, 360.0),
        Panel::Controls => (
            340.0,
            CONTROLS_ROWS.iter().sum::<f32>()
                + apps_h(d.app_volumes.len().min(APP_ROWS))
                + if d.battery.is_some() {
                    BATTERY_ROW
                } else {
                    0.0
                }
                + FOOTER,
        ),
    }
}

pub fn draw(g: &Gfx, st: &State, d: &Data, s: &Settings, w: f32, h: f32) -> Vec<Hit> {
    let dark = crate::ui::theme(s.style).dark;
    let (p, accent) = (pal(dark), crate::ui::accent(s));
    let link = accent_text(s, dark); // the accent as text
    g.fill_rr(0.0, 0.0, w, h, 22.0, p.bg);
    g.stroke_rr(0.0, 0.0, w, h, 22.0, p.border, 1.0);
    let mut hits = vec![];
    let over_close = st.over == Some(Act::Close);
    close_button(g, &p, w - 30.0, 32.0, Act::Close, over_close, &mut hits);
    match st.kind {
        Panel::Calendar => calendar(g, &p, st, d, accent, link, s.clock24, w, h, &mut hits),
        Panel::Weather => weather(g, &p, d, w),
        Panel::Claude | Panel::Codex => ai(g, &p, st.kind, d, w, h),
        Panel::Controls => controls(g, &p, st, d, accent, link, w, h, &mut hits),
        Panel::About => about(g, &p, st, d, accent, link, w, h, &mut hits),
        Panel::Tray => tray(g, &p, st, d, w, h, &mut hits),
    }
    hits
}

/// The wash under the control the cursor is over.
fn hover(g: &Gfx, p: &Pal, st: &State, act: Act, x: f32, y: f32, w: f32, h: f32, r: f32) {
    if st.over == Some(act) {
        g.fill_rr(x, y, w, h, r, wash(p));
    }
}

fn calendar(
    g: &Gfx,
    p: &Pal,
    st: &State,
    d: &Data,
    accent: C,
    link: C,
    h24: bool,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit>,
) {
    let today = d.time.days();
    let (y, m) = st.month;
    g.text(
        &format!("{}, {}", month_name(m), y),
        24.0,
        16.0,
        360.0,
        34.0,
        24.0,
        700,
        p.text,
        Align::Left,
    );

    // header buttons, left of the close button
    hover(g, p, st, Act::Today, w - 196.0, 16.0, 60.0, 32.0, 10.0);
    g.text(
        "Today",
        w - 196.0,
        16.0,
        60.0,
        32.0,
        14.0,
        600,
        link,
        Align::Center,
    );
    hits.push((w - 196.0, 16.0, 60.0, 32.0, Act::Today));
    for (x, act, flip) in [
        (w - 122.0, Act::PrevMonth, -1.0),
        (w - 86.0, Act::NextMonth, 1.0),
    ] {
        let (cx, cy) = (x + 14.0, 32.0);
        hover(g, p, st, act, x, 16.0, 28.0, 32.0, 10.0);
        g.line(cx - 3.0 * flip, cy - 6.0, cx + 3.0 * flip, cy, p.text, 2.0);
        g.line(cx + 3.0 * flip, cy, cx - 3.0 * flip, cy + 6.0, p.text, 2.0);
        hits.push((x, 16.0, 28.0, 32.0, act));
    }

    // weekday pills, Monday first
    let (gx, cw, ch, gap) = (24.0, 54.0, 50.0, 6.0);
    let today_wd = (weekday(today) + 6) % 7;
    let (ty, tm, _) = civil_from_days(today);
    for (i, name) in ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"]
        .iter()
        .enumerate()
    {
        let x = gx + i as f32 * (cw + gap);
        g.fill_rr(x, 70.0, cw, 26.0, 13.0, p.card);
        let bold = i as u32 == today_wd && (ty, tm) == (y, m);
        g.text(
            name,
            x,
            70.0,
            cw,
            26.0,
            11.0,
            if bold { 700 } else { 500 },
            if bold { p.text } else { p.sub },
            Align::Center,
        );
    }

    let first = days_from_civil(y, m, 1);
    let lead = (weekday(first) + 6) % 7;
    let (ny, nm) = add_month((y, m), 1);
    let len = days_from_civil(ny, nm, 1) - first;
    for i in 0..len {
        let day = first + i;
        let slot = lead as i64 + i;
        let (x, cy) = (
            gx + (slot % 7) as f32 * (cw + gap),
            104.0 + (slot / 7) as f32 * (ch + gap),
        );
        g.fill_rr(x, cy, cw, ch, 14.0, p.card);
        hover(g, p, st, Act::Select(day), x, cy, cw, ch, 14.0);
        if day == today {
            g.stroke_rr(x, cy, cw, ch, 14.0, accent, 2.0);
        } else if day == st.selected {
            g.stroke_rr(x, cy, cw, ch, 14.0, p.sub, 1.5);
        }
        let color = if day == today {
            link
        } else if day < today {
            p.sub
        } else {
            p.text
        };
        g.text(
            &(i + 1).to_string(),
            x,
            cy + ch - 26.0,
            cw,
            22.0,
            15.0,
            if day >= today { 600 } else { 400 },
            color,
            Align::Center,
        );
        if d.events.iter().any(|e| e.at.day == day) {
            g.fill_circle(x + cw - 10.0, cy + 10.0, 3.5, accent);
        }
        hits.push((x, cy, cw, ch, Act::Select(day)));
    }

    // the selected day's events
    let (sx, sy, sw, sh) = (
        gx + 7.0 * (cw + gap) + 10.0,
        70.0,
        w - (gx + 7.0 * (cw + gap) + 10.0) - 20.0,
        h - 90.0,
    );
    g.fill_rr(sx, sy, sw, sh, 18.0, p.card);
    let (_, sm, sd) = civil_from_days(st.selected);
    let title = if st.selected == today {
        "Today".to_string()
    } else {
        format!(
            "{} {} {}",
            day_name(weekday(st.selected)),
            sd,
            month_abbr(sm)
        )
    };
    g.text(
        &title,
        sx + 16.0,
        sy + 12.0,
        sw - 32.0,
        24.0,
        16.0,
        700,
        p.text,
        Align::Left,
    );
    let todays: Vec<_> = d
        .events
        .iter()
        .filter(|e| e.at.day == st.selected)
        .collect();
    let count = match todays.len() {
        0 => "No events".to_string(),
        1 => "1 event".to_string(),
        n => format!("{n} events"),
    };
    g.text(
        &count,
        sx + 16.0,
        sy + 36.0,
        sw - 32.0,
        16.0,
        11.0,
        500,
        p.sub,
        Align::Left,
    );
    let mut ey = sy + 62.0;
    for e in todays.iter().take(((sh - 70.0) / 46.0) as usize) {
        let when = e.at.sec.map_or("All day".to_string(), |s| {
            fmt_hm((s / 3600) as u32, (s / 60 % 60) as u32, h24)
        });
        g.text(
            &when,
            sx + 16.0,
            ey,
            sw - 32.0,
            14.0,
            10.5,
            500,
            p.sub,
            Align::Left,
        );
        g.fill_rr(sx + 16.0, ey + 17.0, 3.0, 22.0, 1.5, accent);
        g.text(
            &e.summary,
            sx + 26.0,
            ey + 15.0,
            sw - 42.0,
            16.0,
            12.5,
            600,
            p.text,
            Align::Left,
        );
        if !e.location.is_empty() {
            g.text(
                &e.location,
                sx + 26.0,
                ey + 30.0,
                sw - 42.0,
                13.0,
                10.0,
                400,
                p.sub,
                Align::Left,
            );
        }
        ey += 46.0;
    }
    if d.events.is_empty() {
        g.text(
            "Sign in with Google or add an iCal",
            sx + 16.0,
            sy + sh - 48.0,
            sw - 32.0,
            16.0,
            10.5,
            500,
            p.sub,
            Align::Left,
        );
        g.text(
            "link in Settings (right-click the bar)",
            sx + 16.0,
            sy + sh - 32.0,
            sw - 32.0,
            16.0,
            10.5,
            500,
            p.sub,
            Align::Left,
        );
    }
}

fn weather(g: &Gfx, p: &Pal, d: &Data, w: f32) {
    let Some(wx) = &d.weather else {
        g.text(
            "No weather yet",
            24.0,
            20.0,
            w - 70.0,
            24.0,
            16.0,
            700,
            p.text,
            Align::Left,
        );
        g.text(
            "Set a city in Settings, from the bar's right-click menu.",
            24.0,
            48.0,
            w - 48.0,
            18.0,
            11.5,
            500,
            p.sub,
            Align::Left,
        );
        return;
    };
    let (label, icon) = crate::weather::wmo(wx.code, wx.day);
    g.text(
        &wx.city,
        24.0,
        18.0,
        w - 170.0,
        24.0,
        17.0,
        700,
        p.text,
        Align::Left,
    );
    g.text(
        label,
        24.0,
        42.0,
        w - 170.0,
        18.0,
        12.0,
        500,
        p.sub,
        Align::Left,
    );
    g.text(
        &format!("{:.0}°", wx.temp),
        w - 150.0,
        12.0,
        70.0,
        54.0,
        38.0,
        300,
        p.text,
        Align::Center,
    );
    g.text(
        icon,
        w - 88.0,
        14.0,
        40.0,
        50.0,
        28.0,
        400,
        p.text,
        Align::Center,
    );

    // next six hours
    g.fill_rr(16.0, 78.0, w - 32.0, 86.0, 16.0, p.card);
    let col = (w - 48.0) / 6.0;
    for (i, hr) in wx.hourly.iter().take(6).enumerate() {
        let x = 24.0 + i as f32 * col;
        let label = if i == 0 {
            "Now".to_string()
        } else {
            format!("{}", hr.hour)
        };
        g.text(&label, x, 86.0, col, 16.0, 10.5, 500, p.sub, Align::Center);
        g.text(
            crate::weather::wmo(hr.code, hr.day).1,
            x,
            104.0,
            col,
            28.0,
            18.0,
            400,
            p.text,
            Align::Center,
        );
        g.text(
            &format!("{:.0}°", hr.temp),
            x,
            136.0,
            col,
            18.0,
            12.5,
            600,
            p.text,
            Align::Center,
        );
    }

    // seven days, each with its range drawn against the week's range
    let lo = wx.daily.iter().map(|d| d.min).fold(f32::MAX, f32::min);
    let hi = wx.daily.iter().map(|d| d.max).fold(f32::MIN, f32::max);
    let span = (hi - lo).max(1.0);
    for (
        i,
        &crate::weather::Day {
            weekday: wd,
            code,
            min,
            max,
        },
    ) in wx.daily.iter().take(7).enumerate()
    {
        let y = 178.0 + i as f32 * 30.0;
        let name = if i == 0 { "Today" } else { day_name(wd) };
        g.text(name, 24.0, y, 70.0, 26.0, 12.5, 500, p.text, Align::Left);
        g.text(
            crate::weather::wmo(code, true).1,
            92.0,
            y,
            30.0,
            26.0,
            15.0,
            400,
            p.text,
            Align::Center,
        );
        g.text(
            &format!("{min:.0}°"),
            128.0,
            y,
            36.0,
            26.0,
            12.0,
            500,
            p.sub,
            Align::Left,
        );
        let (bx, bw) = (166.0, w - 166.0 - 64.0);
        g.fill_rr(bx, y + 11.0, bw, 4.0, 2.0, p.card);
        let (a, b) = ((min - lo) / span, (max - lo) / span);
        g.fill_rr_grad(
            bx + bw * a,
            y + 11.0,
            (bw * (b - a)).max(4.0),
            4.0,
            2.0,
            &[(0.0, rgba(0x5AC8FA, 1.0)), (1.0, rgba(0xFFD60A, 1.0))],
            true,
        );
        g.text(
            &format!("{max:.0}°"),
            w - 56.0,
            y,
            36.0,
            26.0,
            12.0,
            600,
            p.text,
            Align::Left,
        );
    }
}

fn ai(g: &Gfx, p: &Pal, kind: Panel, d: &Data, w: f32, h: f32) {
    let name = if kind == Panel::Claude {
        "Claude"
    } else {
        "Codex"
    };
    ai_logo(g, kind, 22.0, 18.0, 34.0);
    g.text(
        name,
        66.0,
        16.0,
        w - 120.0,
        22.0,
        16.0,
        700,
        p.text,
        Align::Left,
    );
    g.text(
        "Plan usage",
        66.0,
        37.0,
        w - 120.0,
        16.0,
        11.0,
        500,
        p.sub,
        Align::Left,
    );
    let rows = limits(kind, d);
    if rows.is_empty() {
        let hint = if kind == Panel::Claude {
            "Sign in to Claude Code on this PC."
        } else {
            "Run Codex CLI once on this PC."
        };
        g.text(
            hint,
            22.0,
            80.0,
            w - 44.0,
            18.0,
            12.0,
            500,
            p.sub,
            Align::Left,
        );
    }
    for (i, l) in rows.iter().enumerate() {
        let y = AI_TOP + i as f32 * AI_ROW;
        g.text(
            l.window.label(),
            22.0,
            y,
            w - 100.0,
            18.0,
            13.0,
            600,
            p.text,
            Align::Left,
        );
        g.text(
            &format!("{:.0}%", l.pct),
            w - 70.0,
            y,
            48.0,
            18.0,
            13.0,
            600,
            p.text,
            Align::Left,
        );
        g.fill_rr(22.0, y + 23.0, w - 44.0, 7.0, 3.5, p.card);
        g.fill_rr(
            22.0,
            y + 23.0,
            (w - 44.0) * (l.pct / 100.0).clamp(0.0, 1.0),
            7.0,
            3.5,
            limit_color(l.pct),
        );
        let reset = l.resets.map_or("No reset time reported".to_string(), |r| {
            format!("Resets in {}", crate::dates::fmt_left(r - d.now))
        });
        g.text(
            &reset,
            22.0,
            y + 34.0,
            w - 44.0,
            15.0,
            10.5,
            500,
            p.sub,
            Align::Left,
        );
    }
    let src = if kind == Panel::Claude {
        "From Claude Code's sign-in on this PC"
    } else {
        "From Codex CLI's session logs"
    };
    g.text(
        src,
        22.0,
        h - AI_SOURCE + 4.0,
        w - 44.0,
        16.0,
        10.0,
        500,
        p.sub,
        Align::Left,
    );
}

/// Our own quick settings: what's playing, Wi-Fi, Bluetooth, mute, volume and battery,
/// with a link to Windows' panel for the rest.
fn controls(
    g: &Gfx,
    p: &Pal,
    st: &State,
    d: &Data,
    accent: C,
    link: C,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit>,
) {
    let (x, cw) = (16.0, w - 32.0);
    g.text(
        "Quick controls",
        22.0,
        16.0,
        w - 80.0,
        30.0,
        16.0,
        700,
        p.text,
        Align::Left,
    );

    // now playing
    let mut y = CONTROLS_ROWS[0];
    g.fill_rr(x, y, cw, 104.0, 16.0, p.card);
    let (title, artist, playing) = match &d.media {
        Some(m) => (m.title.as_str(), m.artist.as_str(), m.playing),
        None => ("Nothing playing", "", false),
    };
    g.text(
        title,
        x + 16.0,
        y + 12.0,
        cw - 32.0,
        20.0,
        13.5,
        700,
        p.text,
        Align::Left,
    );
    g.text(
        artist,
        x + 16.0,
        y + 32.0,
        cw - 32.0,
        16.0,
        11.0,
        500,
        p.sub,
        Align::Left,
    );
    let fade = if d.media.is_some() { 1.0 } else { 0.4 };
    for (i, act) in [Action::Prev, Action::Toggle, Action::Next]
        .into_iter()
        .enumerate()
    {
        let cx = x + cw / 2.0 + (i as f32 - 1.0) * 56.0;
        if d.media.is_some() {
            hover(
                g,
                p,
                st,
                Act::Do(act),
                cx - 24.0,
                y + 56.0,
                48.0,
                40.0,
                12.0,
            );
        }
        media_glyph(g, act, playing, cx, y + 76.0, 7.0, C { a: fade, ..p.text });
        hits.push((cx - 24.0, y + 56.0, 48.0, 40.0, Act::Do(act)));
    }

    // toggles, lit with the accent when on. VPN shows on for VPN apps too, but only Windows
    // VPN profiles can be switched from here
    y += CONTROLS_ROWS[1];
    let muted = d.volume.as_ref().is_some_and(|v| v.muted);
    let bw = (cw - 16.0) / 3.0;
    let toggles = [
        ("Wi-Fi", Action::Wifi),
        ("Bluetooth", Action::Bluetooth),
        ("VPN", Action::Vpn),
        ("Airplane", Action::Airplane),
        ("Hotspot", Action::Hotspot),
        ("Mute", Action::Mute),
    ];
    let row_y = y;
    for (i, (label, act)) in toggles.into_iter().enumerate() {
        let on = if act == Action::Mute {
            Some(muted)
        } else {
            d.radio(act)
        };
        let bx = x + (i % 3) as f32 * (bw + 8.0);
        let y = row_y + (i / 3) as f32 * 80.0;
        let lit = on == Some(true);
        g.fill_rr(bx, y, bw, 72.0, 14.0, if lit { accent } else { p.card });
        hover(g, p, st, Act::Do(act), bx, y, bw, 72.0, 14.0);
        let fg = C {
            a: if on.is_some() { 1.0 } else { 0.35 },
            ..if lit { on_fill(accent) } else { p.text }
        };
        let (cx, cy) = (bx + bw / 2.0, y + 28.0);
        if act == Action::Mute {
            speaker_glyph(g, cx, cy, 20.0, true, fg)
        } else {
            radio_glyph(g, act, cx, cy, 13.0, fg)
        }
        let weight = if lit { 700 } else { 600 }; // bold keeps white on the accent above 3:1
        g.text(
            label,
            bx,
            y + 46.0,
            bw,
            18.0,
            11.0,
            weight,
            fg,
            Align::Center,
        );
        hits.push((bx, y, bw, 72.0, Act::Do(act)));
    }

    // volume, dragged or clicked
    y += CONTROLS_ROWS[2];
    g.fill_rr(x, y, cw, 70.0, 14.0, p.card);
    let (name, level) = d
        .volume
        .as_ref()
        .map_or(("No output device", 0.0), |v| (v.label(), v.level));
    g.text(
        name,
        x + 16.0,
        y + 8.0,
        cw - 90.0,
        20.0,
        11.5,
        600,
        p.sub,
        Align::Left,
    );
    let pct = if muted {
        "Muted".to_string()
    } else {
        format!("{:.0}%", level * 100.0)
    };
    g.text(
        &pct,
        x + cw - 70.0,
        y + 8.0,
        54.0,
        20.0,
        11.5,
        600,
        p.text,
        Align::Right,
    );
    speaker_glyph(g, x + 26.0, y + 46.0, 18.0, muted, p.text);
    let s = (x + 50.0, cw - 72.0, y + 46.0);
    slider(g, p, accent, s, level, muted, Action::Volume, hits);
    y += CONTROLS_ROWS[3];

    // each app's own volume, under the device's
    let apps = &d.app_volumes[..d.app_volumes.len().min(st.app_rows)];
    if !apps.is_empty() {
        g.fill_rr(x, y - 6.0, cw, apps_h(apps.len()) - 12.0, 14.0, p.card);
        for (i, a) in apps.iter().enumerate() {
            let ry = y + i as f32 * APP_ROW;
            let mute = Act::Do(Action::AppMute(i));
            hover(g, p, st, mute, x + 6.0, ry + 4.0, 40.0, APP_ROW - 8.0, 10.0);
            speaker_glyph(g, x + 26.0, ry + APP_ROW / 2.0, 15.0, a.muted, p.text);
            hits.push((x + 6.0, ry + 4.0, 40.0, APP_ROW - 8.0, mute));
            g.text(
                &a.name,
                x + 50.0,
                ry + 2.0,
                cw - 130.0,
                18.0,
                11.0,
                600,
                p.sub,
                Align::Left,
            );
            let pct = if a.muted {
                "Muted".to_string()
            } else {
                format!("{:.0}%", a.level * 100.0)
            };
            g.text(
                &pct,
                x + cw - 70.0,
                ry + 2.0,
                54.0,
                18.0,
                11.0,
                600,
                p.text,
                Align::Right,
            );
            let s = (x + 50.0, cw - 72.0, ry + 30.0);
            slider(
                g,
                p,
                accent,
                s,
                a.level,
                a.muted,
                Action::AppVolume(i),
                hits,
            );
        }
        y += apps_h(apps.len());
    }

    if let Some((f, charging)) = d.battery {
        let state = if charging { "Charging" } else { "On battery" };
        g.text(
            &format!("Battery {:.0}%  ·  {state}", f * 100.0),
            22.0,
            y,
            cw,
            20.0,
            12.0,
            500,
            p.text,
            Align::Left,
        );
    }

    let fy = h - FOOTER;
    let quick = Act::Do(Action::QuickSettings);
    hover(g, p, st, quick, 16.0, fy + 4.0, 190.0, 32.0, 10.0);
    g.text(
        "Windows Quick Settings",
        22.0,
        fy + 6.0,
        200.0,
        28.0,
        12.0,
        600,
        link,
        Align::Left,
    );
    hits.push((16.0, fy + 4.0, 190.0, 32.0, quick));
    let system = Act::Do(Action::SystemSettings);
    hover(g, p, st, system, w - 104.0, fy + 4.0, 88.0, 32.0, 10.0);
    g.text(
        "Settings",
        w - 100.0,
        fy + 6.0,
        78.0,
        28.0,
        12.0,
        600,
        p.sub,
        Align::Right,
    );
    hits.push((w - 104.0, fy + 4.0, 88.0, 32.0, system));
}

fn about(
    g: &Gfx,
    p: &Pal,
    st: &State,
    d: &Data,
    accent: C,
    link: C,
    w: f32,
    h: f32,
    hits: &mut Vec<Hit>,
) {
    // the mark: a small pill dock on an accent square
    let (mx, my, ms) = (24.0, 24.0, 56.0);
    g.fill_rr_grad(
        mx,
        my,
        ms,
        ms,
        14.0,
        &[
            (0.0, C { a: 1.0, ..accent }),
            (
                1.0,
                C {
                    r: accent.r * 0.6,
                    g: accent.g * 0.6,
                    b: accent.b * 0.6,
                    a: 1.0,
                },
            ),
        ],
        true,
    );
    g.fill_rr(
        mx + 10.0,
        my + 34.0,
        ms - 20.0,
        10.0,
        5.0,
        rgba(0xFFFFFF, 0.9),
    );
    for i in 0..3 {
        g.fill_circle(mx + 19.0 + i as f32 * 9.0, my + 39.0, 2.2, accent);
    }
    g.text(
        "serenebar",
        94.0,
        26.0,
        w - 140.0,
        28.0,
        20.0,
        700,
        p.text,
        Align::Left,
    );
    g.text(
        &format!("Version {}", env!("CARGO_PKG_VERSION")),
        94.0,
        54.0,
        w - 140.0,
        18.0,
        12.0,
        500,
        p.sub,
        Align::Left,
    );
    if let Some(r) = &d.update {
        let t = format!("{} is out. Get it", r.version);
        let tw = g.measure(&t, 12.0, 600);
        hover(
            g,
            p,
            st,
            Act::Do(Action::Update),
            90.0,
            70.0,
            tw + 8.0,
            22.0,
            6.0,
        );
        g.text(&t, 94.0, 72.0, tw, 18.0, 12.0, 600, link, Align::Left);
        hits.push((94.0, 72.0, tw, 18.0, Act::Do(Action::Update)));
    }

    let lines = [
        "A native dock for Windows 10 and 11.",
        "Rust and Direct2D, no browser engine inside.",
        "",
        "Weather from Open-Meteo, updates from GitHub.",
        "Claude and Codex limits are read from their",
        "apps on this PC.",
    ];
    for (i, l) in lines.iter().enumerate() {
        g.text(
            l,
            24.0,
            104.0 + i as f32 * 20.0,
            w - 48.0,
            20.0,
            12.0,
            500,
            p.text,
            Align::Left,
        );
    }
    g.fill_rr(24.0, h - 92.0, w - 48.0, 40.0, 12.0, p.card);
    let folder = Act::Do(Action::SettingsFolder);
    hover(g, p, st, folder, 24.0, h - 92.0, w - 48.0, 40.0, 12.0);
    g.text(
        "Open settings folder",
        24.0,
        h - 92.0,
        w - 48.0,
        40.0,
        12.5,
        600,
        link,
        Align::Center,
    );
    hits.push((24.0, h - 92.0, w - 48.0, 40.0, folder));
    g.text(
        "Inspired by lumabar.",
        24.0,
        h - 40.0,
        w - 48.0,
        18.0,
        10.5,
        500,
        p.sub,
        Align::Center,
    );
}

/// Notification-area icons in a grid. The name of the one under the cursor shows at the bottom.
fn tray(g: &Gfx, p: &Pal, st: &State, d: &Data, w: f32, h: f32, hits: &mut Vec<Hit>) {
    g.text(
        "Hidden icons",
        20.0,
        16.0,
        w - 70.0,
        28.0,
        15.0,
        700,
        p.text,
        Align::Left,
    );
    if d.tray.is_empty() {
        g.text(
            "No icons yet",
            20.0,
            64.0,
            w - 40.0,
            40.0,
            12.0,
            500,
            p.sub,
            Align::Left,
        );
    }
    for (i, icon) in d.tray.iter().enumerate() {
        let (x, y) = (
            14.0 + (i % TRAY_COLS) as f32 * TRAY_CELL,
            TRAY_TOP + (i / TRAY_COLS) as f32 * TRAY_CELL,
        );
        if st.hover == Some(i) {
            g.fill_rr(
                x + 3.0,
                y + 3.0,
                TRAY_CELL - 6.0,
                TRAY_CELL - 6.0,
                10.0,
                p.card,
            );
        }
        match g.cached(&icon.key()) {
            Some(b) => g.bitmap(&b, x + 11.0, y + 11.0, 24.0, 24.0),
            None => g.fill_rr(x + 11.0, y + 11.0, 24.0, 24.0, 6.0, p.card),
        }
        hits.push((x, y, TRAY_CELL, TRAY_CELL, Act::TrayIcon(i)));
    }
    let tip = st
        .hover
        .and_then(|i| d.tray.get(i))
        .map_or("Click to open, right-click for its menu", |t| {
            t.tip.lines().next().unwrap_or("")
        });
    let color = if st.hover.is_some() { p.text } else { p.sub };
    g.text(
        tip,
        20.0,
        h - TRAY_TIP + 12.0,
        w - 40.0,
        22.0,
        11.0,
        500,
        color,
        Align::Left,
    );
}

/// A volume slider along `(x, width, y)`, dragged or clicked. Muted, it shows its level greyed.
fn slider(
    g: &Gfx,
    p: &Pal,
    accent: C,
    (tx, tw, ty): (f32, f32, f32),
    level: f32,
    muted: bool,
    act: Action,
    hits: &mut Vec<Hit>,
) {
    let level = level.clamp(0.0, 1.0);
    g.fill_rr(tx, ty - 2.5, tw, 5.0, 2.5, p.border);
    let fill = if muted { p.sub } else { accent };
    g.fill_rr(tx, ty - 2.5, tw * level, 5.0, 2.5, fill);
    g.fill_circle(tx + tw * level, ty, 9.0, rgba(0xFFFFFF, 1.0));
    g.arc(tx + tw * level, ty, 9.0, 1.0, p.border, 1.0);
    hits.push((
        tx - SLIDER_SLACK,
        ty - 16.0,
        tw + 2.0 * SLIDER_SLACK,
        32.0,
        Act::Do(act),
    ));
}
