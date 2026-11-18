//! The settings window, drawn with Direct2D like the bar. DIPs throughout.
use crate::gfx::{rgba, Align, Gfx, C};
use crate::settings::{
    move_row, widget_rows, Control, Settings, Size, Style, WidgetRow, ACCENTS, GROUPS,
};
use crate::ui::{self, pal, Pal};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Field {
    City,
    Calendar,
    GoogleId,
    GoogleSecret,
    DeepSeek,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    Close,
    Quit,
    Style(Style),
    Accent(usize),
    Size(Size),
    Top(bool),
    Full(bool),
    EveryMonitor(bool),
    HideTaskbar(bool),
    Clock24(bool),
    Fahrenheit(bool),
    Startup,
    Screenshots,
    Remove(usize),
    Add(usize), // a group's +: pick a hidden widget to show there
    Drag(usize),
    Control(Control),
    Focus(Field),
    Pick(usize),
    Tab(Tab),
    About,
    GoogleSignIn,
    GoogleSignOut,
    GoogleGuide,
    Update,
    CheckUpdate,
    AutoUpdate,
}

pub type Hit = ui::Hit<Act>;

/// The pages of the window, one shown at a time.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Appearance,
    Dock,
    Widgets,
    Feeds,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Appearance, Tab::Dock, Tab::Widgets, Tab::Feeds];

    fn title(self) -> &'static str {
        ["Appearance", "Dock", "Widgets", "Feeds & accounts"][self as usize]
    }
}

/// What the window is editing. Text fields are applied when you press Enter or leave them.
pub struct State {
    pub tab: Tab,
    pub rows: Vec<WidgetRow>,
    fields: [String; 5], // indexed by Field
    pub focus: Option<Field>,
    caret: usize,     // in chars
    select_all: bool, // Ctrl+A: the next edit replaces the whole field
    drag: Option<RowDrag>,
    pub places: Vec<crate::weather::Place>, // suggestions under the city field
    pub over: Option<Act>,                  // the control under the cursor
}

/// A widget row held by its grip. `moved` turns a press into a drag after a few pixels.
struct RowDrag {
    row: usize,
    y: f32,
    grab: f32,
    moved: bool,
}

const LIST_TOP: f32 = 100.0;
const SUGGESTION: f32 = 28.0;
const CITY_Y: f32 = 104.0; // the City field, which its suggestions hang from
const ROW: f32 = 30.0;

/// Where the widget list goes: the top of every group name, the top of every shown row (indexed
/// like `rows`) and the places a dragged row can land: (line y, group, position among that
/// group's other shown rows). Empty groups keep one row of room. Hidden rows aren't listed.
struct ListLayout {
    heads: [f32; GROUPS],
    rows: Vec<f32>,
    gaps: Vec<(f32, usize, usize)>,
}

const GROUP_HEAD: f32 = 22.0;

fn widget_layout(rows: &[WidgetRow]) -> ListLayout {
    let mut ys = vec![0.0; rows.len()];
    let mut heads = [0.0; GROUPS];
    let mut gaps = vec![];
    let mut y = LIST_TOP;
    for (g, head) in heads.iter_mut().enumerate() {
        *head = y;
        y += GROUP_HEAD;
        let members: Vec<usize> = (0..rows.len())
            .filter(|&i| rows[i].on && rows[i].group == g)
            .collect();
        for (k, &i) in members.iter().enumerate() {
            gaps.push((y, g, k));
            ys[i] = y;
            y += ROW;
        }
        if members.is_empty() {
            gaps.push((y + ROW / 2.0, g, 0));
            y += ROW;
        } else {
            gaps.push((y, g, members.len()));
        }
        y += 4.0;
    }
    ListLayout {
        heads,
        rows: ys,
        gaps,
    }
}

fn nearest_gap(rows: &[WidgetRow], y: f32) -> (f32, usize, usize) {
    *widget_layout(rows)
        .gaps
        .iter()
        .min_by(|a, b| (a.0 - y).abs().total_cmp(&(b.0 - y).abs()))
        .unwrap()
}

/// Where a drag over `y` would drop `row`: (group, position among the group's other rows).
fn drop_target(rows: &[WidgetRow], row: usize, y: f32) -> (usize, usize) {
    let (_, g, k) = nearest_gap(rows, y);
    // the gaps count the dragged row itself; positions after it shift up by one
    let mine = (0..rows.len())
        .filter(|&i| rows[i].on && rows[i].group == g)
        .position(|i| i == row);
    (
        g,
        if mine.is_some_and(|m| k > m) {
            k - 1
        } else {
            k
        },
    )
}

impl State {
    pub fn new(s: &Settings) -> Self {
        State {
            tab: Tab::Appearance,
            rows: widget_rows(&s.widgets),
            fields: [
                s.city.clone(),
                s.calendar.clone(),
                s.google_id.clone(),
                s.google_secret.clone(),
                crate::system::unprotect(&s.deepseek_key),
            ],
            focus: None,
            caret: 0,
            select_all: false,
            drag: None,
            places: vec![],
            over: None,
        }
    }

    /// Puts the text fields into the settings.
    pub fn write_fields(&self, s: &mut Settings) {
        let city = self.get(Field::City);
        if city != s.city {
            (s.lat, s.lon) = (None, None);
        } // a typed city replaces a pinned spot
        s.city = city.to_string();
        s.calendar = self.get(Field::Calendar).to_string();
        s.google_id = self.get(Field::GoogleId).to_string();
        s.google_secret = self.get(Field::GoogleSecret).to_string();
        // encrypting gives new bytes each time, so only a changed key is written
        let key = self.get(Field::DeepSeek);
        if key != crate::system::unprotect(&s.deepseek_key) {
            s.deepseek_key = if key.is_empty() {
                String::new()
            } else {
                crate::system::protect(key)
            };
        }
    }

    /// A text field as typed, trimmed.
    pub fn get(&self, f: Field) -> &str {
        self.fields[f as usize].trim()
    }

    fn text(&mut self, f: Field) -> &mut String {
        &mut self.fields[f as usize]
    }

    pub fn focus(&mut self, f: Field) {
        if self.focus == Some(f) {
            return;
        }
        self.focus = Some(f);
        self.caret = self.text(f).chars().count();
        self.select_all = false;
    }

    /// A WM_CHAR for the focused field: text, Backspace, Ctrl+A, or Ctrl+V with `paste`.
    /// True when it was Enter, which applies the field.
    pub fn type_char(&mut self, ch: u32, paste: impl FnOnce() -> Option<String>) -> bool {
        let Some(f) = self.focus else { return false };
        if ch == 0x0D {
            return true;
        }
        let mut chars: Vec<char> = self.text(f).chars().collect();
        if self.select_all && (ch == 0x08 || ch == 0x16 || ch >= 0x20) {
            chars.clear();
            self.caret = 0;
            self.select_all = false;
        }
        match ch {
            0x01 => self.select_all = true,
            0x08 => {
                if self.caret > 0 {
                    self.caret -= 1;
                    chars.remove(self.caret);
                }
            }
            0x16 => {
                for c in paste()
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| !c.is_control())
                {
                    chars.insert(self.caret, c);
                    self.caret += 1;
                }
            }
            c if c >= 0x20 => {
                if let Some(c) = char::from_u32(c) {
                    chars.insert(self.caret, c);
                    self.caret += 1;
                }
            }
            _ => {}
        }
        *self.text(f) = chars.into_iter().collect();
        false
    }

    /// Caret keys and Delete in the focused field. False for any other key.
    pub fn key(&mut self, vk: VIRTUAL_KEY) -> bool {
        let Some(f) = self.focus else { return false };
        // Backspace and typing reach here before their WM_CHAR, which still needs the Ctrl+A
        if ![VK_LEFT, VK_RIGHT, VK_HOME, VK_END, VK_DELETE].contains(&vk) {
            return false;
        }
        if std::mem::take(&mut self.select_all) && vk == VK_DELETE {
            self.text(f).clear();
            self.caret = 0;
            return true;
        }
        let len = self.text(f).chars().count();
        match vk {
            VK_LEFT => self.caret = self.caret.saturating_sub(1),
            VK_RIGHT => self.caret = (self.caret + 1).min(len),
            VK_HOME => self.caret = 0,
            VK_END => self.caret = len,
            VK_DELETE if self.caret < len => {
                let mut chars: Vec<char> = self.text(f).chars().collect();
                chars.remove(self.caret);
                *self.text(f) = chars.into_iter().collect();
            }
            _ => return false,
        }
        true
    }

    /// Takes suggestion `i` into the city field and leaves the field.
    pub fn pick(&mut self, i: usize) -> Option<crate::weather::Place> {
        let p = self.places.get(i)?.clone();
        self.fields[Field::City as usize] = p.name.clone();
        self.focus = None;
        self.places.clear();
        Some(p)
    }

    pub fn remove(&mut self, row: usize) {
        self.rows[row].on = false;
    }

    /// Shows hidden row `row` at the end of `group`.
    pub fn add(&mut self, row: usize, group: usize) {
        self.rows[row].on = true;
        move_row(&mut self.rows, row, group, usize::MAX);
    }

    /// Picks up a widget row by its grip, `y` DIPs down the window.
    pub fn press(&mut self, row: usize, y: f32) {
        let top = widget_layout(&self.rows).rows[row];
        self.drag = Some(RowDrag {
            row,
            y,
            grab: y - top,
            moved: false,
        });
    }

    /// Follows the cursor with the held row. True when that needs a redraw.
    pub fn drag_to(&mut self, y: f32) -> bool {
        let Some(d) = &mut self.drag else {
            return false;
        };
        d.moved |= (y - d.y).abs() > 3.0;
        if d.moved {
            d.y = y;
        }
        d.moved
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Lets go of the held row, moving it where it was dropped. True when it moved, false when
    /// the press was a click.
    pub fn drop(&mut self) -> bool {
        let Some(d) = self.drag.take().filter(|d| d.moved) else {
            return false;
        };
        let (group, k) = drop_target(&self.rows, d.row, d.y);
        move_row(&mut self.rows, d.row, group, k);
        true
    }
}

pub struct Flags {
    pub startup: bool,
    pub screenshots: bool,
    pub google: bool,
    pub google_status: String,
    pub google_account: String,
    pub update: Option<String>, // a newer release's version
    pub update_note: &'static str,
}

pub const SIZE: (f32, f32) = (820.0, 690.0);
const SIDEBAR: f32 = 196.0; // the tab list down the left
pub const HEADER: f32 = 64.0; // drag the window by this strip

struct Ctx<'a> {
    p: Pal,
    accent: C,
    link: C, // the accent as text
    over: Option<Act>,
    hits: &'a mut Vec<Hit>,
}

impl Ctx<'_> {
    /// The wash under the control the cursor is over.
    fn hover(&self, g: &Gfx, act: Act, x: f32, y: f32, w: f32, h: f32, r: f32) {
        if self.over == Some(act) {
            g.fill_rr(x, y, w, h, r, ui::wash(&self.p));
        }
    }

    fn title(&self, g: &Gfx, s: &str, x: f32, y: f32) {
        g.text(s, x, y, 300.0, 22.0, 15.0, 700, self.p.text, Align::Left);
    }

    fn label(&self, g: &Gfx, s: &str, x: f32, y: f32, w: f32) {
        g.text(s, x, y, w, 30.0, 12.5, 500, self.p.text, Align::Left);
    }

    /// A row of options, the chosen one filled with the accent.
    fn segmented(&mut self, g: &Gfx, x: f32, y: f32, w: f32, opts: &[(&str, Act, bool)]) {
        g.fill_rr(x, y, w, 30.0, 9.0, self.p.card);
        let sw = w / opts.len() as f32;
        for (i, &(name, act, on)) in opts.iter().enumerate() {
            let sx = x + i as f32 * sw;
            if on {
                g.fill_rr(sx + 2.0, y + 2.0, sw - 4.0, 26.0, 7.0, self.accent);
            } else {
                self.hover(g, act, sx + 2.0, y + 2.0, sw - 4.0, 26.0, 7.0);
            }
            // bold keeps white on the accent above 3:1
            let (fg, weight) = if on {
                (ui::on_fill(self.accent), 700)
            } else {
                (self.p.text, 600)
            };
            g.text(name, sx, y, sw, 30.0, 11.5, weight, fg, Align::Center);
            self.hits.push((sx, y, sw, 30.0, act));
        }
    }

    fn switch(&mut self, g: &Gfx, x: f32, y: f32, on: bool, act: Act) {
        g.fill_rr(
            x,
            y,
            40.0,
            24.0,
            12.0,
            if on { self.accent } else { self.p.card },
        );
        self.hover(g, act, x, y, 40.0, 24.0, 12.0);
        g.stroke_rr(x, y, 40.0, 24.0, 12.0, self.p.border, 1.0);
        // the knob's rim keeps it visible on the light accents and the white card
        let kx = if on { x + 28.0 } else { x + 12.0 };
        g.fill_circle(kx, y + 12.0, 9.0, rgba(0xFFFFFF, 1.0));
        g.arc(kx, y + 12.0, 9.0, 1.0, rgba(0x000000, 0.18), 1.0);
        self.hits.push((x - 4.0, y - 4.0, 48.0, 32.0, act));
    }

    fn field(&mut self, g: &Gfx, st: &State, f: Field, x: f32, y: f32, w: f32, placeholder: &str) {
        let focused = st.focus == Some(f);
        g.fill_rr(x, y, w, 32.0, 9.0, self.p.card);
        g.stroke_rr(
            x,
            y,
            w,
            32.0,
            9.0,
            if focused {
                self.accent
            } else if self.over == Some(Act::Focus(f)) {
                self.p.sub
            } else {
                self.p.border
            },
            if focused { 2.0 } else { 1.0 },
        );
        let raw = &st.fields[f as usize];
        // the secret shows as dots
        let masked = "•".repeat(raw.chars().count());
        let text = if matches!(f, Field::GoogleSecret | Field::DeepSeek) {
            &masked
        } else {
            raw
        };
        let (tx, tw) = (x + 10.0, w - 20.0);
        if text.is_empty() && !focused {
            g.text(
                placeholder,
                tx,
                y,
                tw,
                32.0,
                12.0,
                400,
                self.p.sub,
                Align::Left,
            );
        } else {
            // keep the caret in view by sliding long text left
            let before: String = text.chars().take(st.caret).collect();
            let caret_x = g.measure(&before, 12.0, 400);
            let shift = if focused {
                (caret_x - tw + 4.0).max(0.0)
            } else {
                0.0
            };
            g.clip(tx, y, tw, 32.0);
            if focused && st.select_all && !text.is_empty() {
                let mut sel = self.accent;
                sel.a = 0.35;
                let sel_w = g.measure(text, 12.0, 400);
                g.fill_rr(tx - shift, y + 7.0, sel_w, 18.0, 3.0, sel);
            }
            g.text(
                text,
                tx - shift,
                y,
                10_000.0,
                32.0,
                12.0,
                400,
                self.p.text,
                Align::Left,
            );
            if focused {
                g.fill_rr(tx - shift + caret_x, y + 8.0, 1.5, 16.0, 0.5, self.accent);
            }
            g.unclip();
        }
        self.hits.push((x, y, w, 32.0, Act::Focus(f)));
    }
}

pub fn draw(g: &Gfx, st: &State, s: &Settings, flags: Flags, w: f32, h: f32) -> Vec<Hit> {
    let (dark, accent) = (ui::theme(s.style).dark, ui::accent(s));
    let mut hits = vec![];
    let mut c = Ctx {
        p: pal(dark),
        accent,
        link: ui::accent_text(s, dark),
        over: st.over,
        hits: &mut hits,
    };
    g.fill_rr(0.0, 0.0, w, h, 20.0, c.p.bg);
    // the sidebar: rounded with the window on the left, square where it meets the page
    g.clip(0.0, 0.0, SIDEBAR, h);
    g.fill_rr(0.0, 0.0, SIDEBAR + 20.0, h, 20.0, c.p.card);
    g.unclip();
    g.stroke_rr(0.0, 0.0, w, h, 20.0, c.p.border, 1.0);
    g.text(
        "Settings",
        24.0,
        20.0,
        SIDEBAR - 24.0,
        30.0,
        20.0,
        700,
        c.p.text,
        Align::Left,
    );
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let (x, y, tw) = (12.0, 72.0 + i as f32 * 38.0, SIDEBAR - 24.0);
        let on = st.tab == tab;
        if on {
            g.fill_rr(x, y, tw, 32.0, 9.0, accent);
        } else {
            c.hover(g, Act::Tab(tab), x, y, tw, 32.0, 9.0);
        }
        let (fg, weight) = if on {
            (ui::on_fill(accent), 700)
        } else {
            (c.p.text, 600)
        };
        g.text(
            tab.title(),
            x + 12.0,
            y,
            tw - 12.0,
            32.0,
            12.5,
            weight,
            fg,
            Align::Left,
        );
        c.hits.push((x, y, tw, 32.0, Act::Tab(tab)));
    }
    // a newer release, else the check button or how the last check went
    let (t, col, act) = match (&flags.update, flags.update_note) {
        (Some(v), _) => (format!("Update to {v}"), c.link, Some(Act::Update)),
        (None, "") => ("Check for updates".into(), c.p.sub, Some(Act::CheckUpdate)),
        (None, "Checking…") => ("Checking…".into(), c.p.sub, None),
        (None, note) => (note.into(), c.p.sub, Some(Act::CheckUpdate)), // click to check again
    };
    let y = h - 118.0;
    if let Some(act) = act {
        c.hover(g, act, 16.0, y, SIDEBAR - 32.0, 30.0, 8.0);
    }
    g.text(
        &t,
        24.0,
        y,
        SIDEBAR - 36.0,
        30.0,
        12.5,
        600,
        col,
        Align::Left,
    );
    if let Some(act) = act {
        c.hits.push((16.0, y, SIDEBAR - 32.0, 30.0, act));
    }
    for (i, (name, act, col)) in [
        ("About serenebar", Act::About, c.p.sub),
        ("Quit serenebar", Act::Quit, ui::danger_text(dark)),
    ]
    .into_iter()
    .enumerate()
    {
        let y = h - 84.0 + i as f32 * 34.0;
        c.hover(g, act, 16.0, y, SIDEBAR - 32.0, 30.0, 8.0);
        g.text(
            name,
            24.0,
            y,
            SIDEBAR - 36.0,
            30.0,
            12.5,
            600,
            col,
            Align::Left,
        );
        c.hits.push((16.0, y, SIDEBAR - 32.0, 30.0, act));
    }
    let over_close = c.over == Some(Act::Close);
    ui::close_button(g, &c.p, w - 34.0, 36.0, Act::Close, over_close, c.hits);

    let (x, cw) = (SIDEBAR + 28.0, w - SIDEBAR - 56.0);
    match st.tab {
        Tab::Appearance => appearance(g, &mut c, s, x, cw),
        Tab::Dock => dock(g, &mut c, s, &flags, x, cw),
        Tab::Widgets => widgets(g, &mut c, st, s, x, cw),
        Tab::Feeds => feeds(g, &mut c, st, s, &flags, x, cw),
    }
    hits
}

fn appearance(g: &Gfx, c: &mut Ctx, s: &Settings, x0: f32, cw: f32) {
    c.title(g, "Style", x0, 72.0);
    let chip = (cw - 16.0) / 3.0;
    for (i, style) in Style::ALL.iter().enumerate() {
        let (x, y) = (
            x0 + (i % 3) as f32 * (chip + 8.0),
            104.0 + (i / 3) as f32 * 38.0,
        );
        let on = s.style == *style;
        g.fill_rr(x, y, chip, 30.0, 9.0, c.p.card);
        c.hover(g, Act::Style(*style), x, y, chip, 30.0, 9.0);
        if on {
            g.stroke_rr(x, y, chip, 30.0, 9.0, c.accent, 2.0);
        }
        // swatch: the style's bar colour, or a glass ring for the clear ones
        let th = ui::theme(*style);
        let swatch = th.bar.unwrap_or(rgba(0x000000, 0.0));
        g.fill_circle(
            x + 16.0,
            y + 15.0,
            8.0,
            C {
                a: swatch.a.max(0.35),
                ..swatch
            },
        );
        g.arc(
            x + 16.0,
            y + 15.0,
            8.0,
            1.0,
            if crate::glass::params(*style).is_some() {
                c.accent
            } else {
                c.p.border
            },
            1.5,
        );
        g.text(
            style.title(),
            x + 30.0,
            y,
            chip - 36.0,
            30.0,
            12.0,
            if on { 700 } else { 500 },
            c.p.text,
            Align::Left,
        );
        c.hits.push((x, y, chip, 30.0, Act::Style(*style)));
    }
    let y = 116.0 + Style::ALL.len().div_ceil(3) as f32 * 38.0; // below the style chips
    c.title(g, "Accent", x0, y);
    for (i, (hex, _)) in ACCENTS.iter().enumerate() {
        let (dx, dy) = (x0 + 11.0 + i as f32 * 34.0, y + 44.0);
        if s.accent == i {
            g.arc(dx, dy, 13.0, 1.0, c.p.text, 2.0);
        } else if c.over == Some(Act::Accent(i)) {
            g.fill_circle(dx, dy, 14.0, ui::wash(&c.p));
        }
        g.fill_circle(dx, dy, 10.0, rgba(*hex, 1.0));
        c.hits
            .push((dx - 15.0, dy - 15.0, 30.0, 30.0, Act::Accent(i)));
    }
    c.title(g, "Size", x0, y + 86.0);
    let sizes = Size::ALL.map(|z| (z.label(), Act::Size(z), s.size == z));
    c.segmented(g, x0 + cw - 270.0, y + 82.0, 270.0, &sizes);
}

fn dock(g: &Gfx, c: &mut Ctx, s: &Settings, flags: &Flags, x0: f32, cw: f32) {
    c.title(g, "Dock", x0, 72.0);
    let seg_w = 200.0;
    let sx = x0 + cw - seg_w;
    let rows: [(&str, Act, &str, Act, &str, bool); 5] = [
        (
            "Position",
            Act::Top(false),
            "Bottom",
            Act::Top(true),
            "Top",
            s.top,
        ),
        (
            "Width",
            Act::Full(false),
            "Pill",
            Act::Full(true),
            "Full width",
            s.full,
        ),
        (
            "Monitors",
            Act::EveryMonitor(false),
            "Primary",
            Act::EveryMonitor(true),
            "All",
            s.every_monitor,
        ),
        (
            "Windows taskbar",
            Act::HideTaskbar(false),
            "Auto-hide",
            Act::HideTaskbar(true),
            "Hidden",
            s.hide_taskbar,
        ),
        (
            "Clock",
            Act::Clock24(true),
            "24-hour",
            Act::Clock24(false),
            "12-hour",
            !s.clock24,
        ),
    ];
    for (i, (label, a0, n0, a1, n1, on1)) in rows.into_iter().enumerate() {
        let y = 104.0 + i as f32 * 38.0;
        c.label(g, label, x0, y, cw - seg_w);
        c.segmented(g, sx, y, seg_w, &[(n0, a0, !on1), (n1, a1, on1)]);
    }
    c.title(g, "System", x0, 312.0);
    c.label(g, "Launch at startup", x0, 344.0, cw - 60.0);
    c.switch(g, x0 + cw - 40.0, 347.0, flags.startup, Act::Startup);
    c.label(g, "Show in screenshots", x0, 382.0, cw - 60.0);
    c.switch(
        g,
        x0 + cw - 40.0,
        385.0,
        flags.screenshots,
        Act::Screenshots,
    );
    c.label(g, "Install updates automatically", x0, 438.0, cw - 60.0);
    c.switch(g, x0 + cw - 40.0, 441.0, s.auto_update, Act::AutoUpdate);
    for (t, y) in [
        (
            "Lets screenshots and recordings show the dock. The glass stops updating while it's on.",
            406.0,
        ),
        ("While you're away, so the bar restarts unnoticed", 462.0),
    ] {
        g.text(t, x0, y, cw, 18.0, 10.5, 400, c.p.sub, Align::Left);
    }
}

fn widgets(g: &Gfx, c: &mut Ctx, st: &State, s: &Settings, x0: f32, cw: f32) {
    // left: widgets by group, dragged by their grip
    let w3 = 300.0;
    c.title(g, "Widgets", x0, 72.0);
    let list = widget_layout(&st.rows);
    for (gi, gname) in ["Left", "Centre", "Right"].iter().enumerate() {
        let gy = list.heads[gi];
        g.text(gname, x0, gy, w3, 20.0, 10.5, 600, c.p.sub, Align::Left);
        if st.drag.is_none() && st.rows.iter().any(|r| !r.on) {
            let (bx, by) = (x0 + w3 - 26.0, gy - 1.0);
            g.fill_rr(bx, by, 20.0, 20.0, 6.0, c.p.card);
            c.hover(g, Act::Add(gi), bx, by, 20.0, 20.0, 6.0);
            g.fill_rr(bx + 5.0, by + 9.25, 10.0, 1.5, 0.75, c.p.text);
            g.fill_rr(bx + 9.25, by + 5.0, 1.5, 10.0, 0.75, c.p.text);
            c.hits.push((bx - 4.0, by - 2.0, 28.0, 24.0, Act::Add(gi)));
        }
        if !st.rows.iter().any(|r| r.on && r.group == gi) {
            g.stroke_rr(x0, gy + GROUP_HEAD, w3, ROW - 4.0, 8.0, c.p.border, 1.0);
            g.text(
                "Drag a widget here, or add one with +",
                x0,
                gy + GROUP_HEAD,
                w3,
                ROW - 4.0,
                11.0,
                500,
                c.p.sub,
                Align::Center,
            );
        }
    }
    let held = st.drag.as_ref().filter(|d| d.moved);
    for (i, r) in st.rows.iter().enumerate().filter(|(_, r)| r.on) {
        let lifted = held.is_some_and(|d| d.row == i);
        let y = if lifted {
            held.unwrap().y - held.unwrap().grab
        } else {
            list.rows[i]
        };
        if lifted {
            g.fill_rr(x0 - 6.0, y, w3 + 12.0, ROW, 8.0, c.p.card);
            g.stroke_rr(x0 - 6.0, y, w3 + 12.0, ROW, 8.0, c.accent, 1.5);
        }
        row(g, c, st, i, r, x0, y, w3);
    }
    if let Some(d) = held {
        g.fill_rr(
            x0,
            nearest_gap(&st.rows, d.y).0 - 1.0,
            w3,
            2.0,
            1.0,
            c.accent,
        );
    }

    // right: what the quick controls tile shows
    let (x1, w1) = (x0 + w3 + 32.0, cw - w3 - 32.0);
    c.title(g, "Quick controls tile", x1, 72.0);
    let chip_w = (w1 - 2.0 * 8.0) / 3.0;
    for (i, control) in Control::ALL.into_iter().enumerate() {
        let (x, y) = (
            x1 + (i % 3) as f32 * (chip_w + 8.0),
            104.0 + (i / 3) as f32 * 58.0,
        );
        let on = s.controls.contains(&control);
        let fg = if on { ui::on_fill(c.accent) } else { c.p.text };
        g.fill_rr(
            x,
            y,
            chip_w,
            50.0,
            10.0,
            if on { c.accent } else { c.p.card },
        );
        c.hover(g, Act::Control(control), x, y, chip_w, 50.0, 10.0);
        ui::control_glyph(g, control, x + chip_w / 2.0, y + 18.0, 10.0, fg);
        g.text(
            control.label(),
            x,
            y + 30.0,
            chip_w,
            16.0,
            10.5,
            if on { 700 } else { 600 },
            fg,
            Align::Center,
        );
        c.hits.push((x, y, chip_w, 50.0, Act::Control(control)));
    }
}

fn feeds(g: &Gfx, c: &mut Ctx, st: &State, s: &Settings, flags: &Flags, x0: f32, cw: f32) {
    c.title(g, "Weather", x0, CITY_Y - 32.0);
    c.segmented(
        g,
        x0 + cw - 100.0,
        CITY_Y - 36.0,
        100.0,
        &[
            ("°C", Act::Fahrenheit(false), !s.fahrenheit),
            ("°F", Act::Fahrenheit(true), s.fahrenheit),
        ],
    );
    c.field(
        g,
        st,
        Field::City,
        x0,
        CITY_Y,
        cw,
        "City, e.g. San Francisco",
    );
    c.title(g, "Calendar", x0, 152.0);
    c.field(
        g,
        st,
        Field::Calendar,
        x0,
        182.0,
        cw,
        "Secret iCal link (.ics)",
    );
    g.text(
        "From Google, Outlook or iCloud calendar settings",
        x0,
        218.0,
        cw,
        18.0,
        10.5,
        400,
        c.p.sub,
        Align::Left,
    );

    c.title(g, "Google Calendar and Gmail", x0, 258.0);
    if flags.google {
        let line = if flags.google_account.is_empty() {
            "Signed in. Calendars you tick in Google Calendar show up here.".to_string()
        } else {
            format!(
                "Signed in as {}. Your ticked calendars show up here.",
                flags.google_account
            )
        };
        g.text(&line, x0, 288.0, cw, 18.0, 11.0, 500, c.p.text, Align::Left);
        g.fill_rr(x0, 318.0, 110.0, 32.0, 9.0, c.p.card);
        c.hover(g, Act::GoogleSignOut, x0, 318.0, 110.0, 32.0, 9.0);
        g.text(
            "Sign out",
            x0,
            318.0,
            110.0,
            32.0,
            12.0,
            600,
            c.p.text,
            Align::Center,
        );
        c.hits.push((x0, 318.0, 110.0, 32.0, Act::GoogleSignOut));
    } else {
        let fw = (cw - 8.0) / 2.0;
        c.field(g, st, Field::GoogleId, x0, 288.0, fw, "OAuth client ID");
        c.field(
            g,
            st,
            Field::GoogleSecret,
            x0 + fw + 8.0,
            288.0,
            fw,
            "Client secret",
        );
        let ready = !st.get(Field::GoogleId).is_empty() && !st.get(Field::GoogleSecret).is_empty();
        g.fill_rr(
            x0,
            330.0,
            160.0,
            32.0,
            9.0,
            if ready { c.accent } else { c.p.card },
        );
        c.hover(g, Act::GoogleSignIn, x0, 330.0, 160.0, 32.0, 9.0);
        let (fg, weight) = if ready {
            (ui::on_fill(c.accent), 700)
        } else {
            (c.p.sub, 600)
        };
        g.text(
            "Sign in with Google",
            x0,
            330.0,
            160.0,
            32.0,
            12.0,
            weight,
            fg,
            Align::Center,
        );
        if ready {
            c.hits.push((x0, 330.0, 160.0, 32.0, Act::GoogleSignIn));
        }
    }
    if flags.google_status.is_empty() && !flags.google {
        let note = "Needs your own OAuth client. ";
        let nw = g.measure(note, 10.5, 400);
        g.text(note, x0, 370.0, cw, 18.0, 10.5, 400, c.p.sub, Align::Left);
        let link = "How to make one";
        let lw = g.measure(link, 10.5, 600);
        c.hover(
            g,
            Act::GoogleGuide,
            x0 + nw - 4.0,
            368.0,
            lw + 8.0,
            22.0,
            6.0,
        );
        g.text(
            link,
            x0 + nw,
            370.0,
            lw,
            18.0,
            10.5,
            600,
            c.link,
            Align::Left,
        );
        c.hits.push((x0 + nw, 370.0, lw, 18.0, Act::GoogleGuide));
    } else {
        let note = flags.google_status.as_str();
        g.text(note, x0, 370.0, cw, 18.0, 10.5, 400, c.p.sub, Align::Left);
    }

    c.title(g, "DeepSeek", x0, 414.0);
    c.field(
        g,
        st,
        Field::DeepSeek,
        x0,
        444.0,
        cw,
        "API key, for the balance widget",
    );
    g.text(
        "From platform.deepseek.com. It's kept encrypted for your Windows account.",
        x0,
        480.0,
        cw,
        18.0,
        10.5,
        400,
        c.p.sub,
        Align::Left,
    );

    // city suggestions, over whatever is below the field and ahead of it in the hits
    if !st.places.is_empty() {
        let (x, y0) = (x0, CITY_Y + 36.0);
        let ph = st.places.len() as f32 * SUGGESTION;
        g.fill_rr(x, y0, cw, ph + 8.0, 10.0, c.p.bg);
        g.fill_rr(x, y0, cw, ph + 8.0, 10.0, c.p.card);
        g.stroke_rr(x, y0, cw, ph + 8.0, 10.0, c.p.border, 1.0);
        let mut picks = vec![];
        for (i, p) in st.places.iter().enumerate() {
            let y = y0 + 4.0 + i as f32 * SUGGESTION;
            c.hover(g, Act::Pick(i), x + 4.0, y, cw - 8.0, SUGGESTION, 7.0);
            let nw = g.measure(&p.name, 12.0, 600).min(cw - 24.0);
            g.text(
                &p.name,
                x + 12.0,
                y,
                nw + 1.0,
                SUGGESTION,
                12.0,
                600,
                c.p.text,
                Align::Left,
            );
            g.text(
                &p.area,
                x + 20.0 + nw,
                y,
                cw - 32.0 - nw,
                SUGGESTION,
                11.0,
                400,
                c.p.sub,
                Align::Left,
            );
            picks.push((x, y, cw, SUGGESTION, Act::Pick(i)));
        }
        c.hits.splice(0..0, picks);
    }
}

fn row(g: &Gfx, c: &mut Ctx, st: &State, i: usize, r: &WidgetRow, x: f32, y: f32, w: f32) {
    if st.drag.is_none() {
        c.hover(g, Act::Drag(i), x + 24.0, y + 1.0, w - 24.0, ROW - 2.0, 8.0);
    }
    // remove: a minus in a circle
    g.fill_circle(x + 10.0, y + ROW / 2.0, 9.0, c.p.card);
    if st.drag.is_none() && c.over == Some(Act::Remove(i)) {
        g.fill_circle(x + 10.0, y + ROW / 2.0, 9.0, ui::wash(&c.p));
    }
    g.fill_rr(x + 5.5, y + ROW / 2.0 - 0.75, 9.0, 1.5, 0.75, c.p.sub);
    if st.drag.is_none() {
        c.hits.push((x - 2.0, y, 26.0, ROW, Act::Remove(i)));
    }
    g.text(
        r.widget.label(),
        x + 30.0,
        y,
        w - 68.0,
        ROW,
        12.0,
        500,
        c.p.text,
        Align::Left,
    );
    // grip: two columns of dots
    for (dx, dy) in [
        (0.0, -5.0),
        (0.0, 0.0),
        (0.0, 5.0),
        (5.0, -5.0),
        (5.0, 0.0),
        (5.0, 5.0),
    ] {
        g.fill_circle(x + w - 16.0 + dx, y + ROW / 2.0 + dy, 1.4, c.p.sub);
    }
    if st.drag.is_none() {
        c.hits.push((x + 24.0, y, w - 24.0, ROW, Act::Drag(i)));
    }
}
