//! The settings file and everything the user can choose.
use std::path::PathBuf;

/// Glass styles first, then solid ones, then Bare. The menu and the settings window follow this order.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style {
    Lagoon,
    Frost,
    Dusk,
    Ember,
    Tide,
    Mist,
    Obsidian,
    Slate,
    Porcelain,
    Sand,
    Bare,
}

impl Style {
    pub const ALL: [Style; 11] = [
        Style::Lagoon,
        Style::Frost,
        Style::Dusk,
        Style::Ember,
        Style::Tide,
        Style::Mist,
        Style::Obsidian,
        Style::Slate,
        Style::Porcelain,
        Style::Sand,
        Style::Bare,
    ];
    pub fn name(self) -> &'static str {
        [
            "lagoon",
            "frost",
            "dusk",
            "ember",
            "tide",
            "mist",
            "obsidian",
            "slate",
            "porcelain",
            "sand",
            "bare",
        ][self as usize]
    }
    pub fn title(self) -> &'static str {
        [
            "Lagoon",
            "Frost",
            "Dusk",
            "Ember",
            "Tide",
            "Mist",
            "Obsidian",
            "Slate",
            "Porcelain",
            "Sand",
            "Bare",
        ][self as usize]
    }
    pub fn description(self) -> &'static str {
        [
            "clear glass",
            "frosted glass",
            "dark glass",
            "warm glass",
            "cool glass",
            "light glass",
            "black",
            "blue-grey",
            "white",
            "warm white",
            "tiles only",
        ][self as usize]
    }
    pub fn parse(v: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|x| x.name() == v)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Size {
    S,
    M,
    L,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::S, Size::M, Size::L];
    pub fn id(self) -> &'static str {
        ["s", "m", "l"][self as usize]
    }
    pub fn label(self) -> &'static str {
        ["Small", "Medium", "Large"][self as usize]
    }
    /// Everything on the bar scales by this.
    pub fn scale(self) -> f32 {
        [0.86, 1.0, 1.16][self as usize]
    }
}

/// iOS dark-mode system colours.
pub const ACCENTS: [(u32, &str); 9] = [
    (0x0A84FF, "Blue"),
    (0xBF5AF2, "Purple"),
    (0xFF375F, "Pink"),
    (0xFF453A, "Red"),
    (0xFF9F0A, "Orange"),
    (0xFFD60A, "Yellow"),
    (0x30D158, "Green"),
    (0x64D2FF, "Teal"),
    (0x8E8E93, "Graphite"),
];

/// Each accent as text, (on light surfaces, on dark surfaces), in the order of `ACCENTS`.
/// The fills above are too light to read as text on white, and some too dark on the dark cards.
/// Every one of these is at least 4.5:1 on the panel and card colours of its appearance.
pub const ACCENT_TEXT: [(u32, u32); 9] = [
    (0x0040DD, 0x409CFF),
    (0x8944AB, 0xDA8FFF),
    (0xD30F45, 0xFF6482),
    (0xD70015, 0xFF6961),
    (0xC93400, 0xFF9F0A),
    (0xA05A00, 0xFFD60A),
    (0x1E7D34, 0x30D158),
    (0x0071A4, 0x64D2FF),
    (0x6C6C70, 0xAEAEB2),
];

/// Red text for actions like Quit, (light, dark).
pub const DANGER_TEXT: (u32, u32) = ACCENT_TEXT[3];

/// Relative luminance of an sRGB colour, as WCAG defines it.
pub fn luminance(hex: u32) -> f32 {
    let lin = |c: u32| {
        let c = (c & 255) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(hex >> 16) + 0.7152 * lin(hex >> 8) + 0.0722 * lin(hex)
}

/// Whether text and glyphs on a fill of `hex` should be near-black instead of white.
/// White falls below 3:1 on Orange, Yellow, Green and Teal.
pub fn dark_text_on(hex: u32) -> bool {
    luminance(hex) > 0.3
}

/// Tiles the dock can show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widget {
    Weather,
    Media,
    System,
    Start,
    Apps,
    Claude,
    Codex,
    Controls,
    Battery,
    Tray,
    Lang,
    Desktop,
    Clock,
    Mic,
    Network,
    Theme,
    Progress,
    Antigravity,
    DeepSeek,
    Cursor,
    Gmail,
    Notifications,
}

impl Widget {
    /// In the order the settings window lists them.
    /// Antigravity isn't offered yet: its numbers didn't match the app's on a Starter plan.
    pub const ALL: [Widget; 21] = [
        Widget::Weather,
        Widget::Media,
        Widget::System,
        Widget::Start,
        Widget::Apps,
        Widget::Claude,
        Widget::Codex,
        Widget::Cursor,
        Widget::DeepSeek,
        Widget::Gmail,
        Widget::Controls,
        Widget::Battery,
        Widget::Tray,
        Widget::Lang,
        Widget::Desktop,
        Widget::Notifications,
        Widget::Clock,
        Widget::Mic,
        Widget::Network,
        Widget::Theme,
        Widget::Progress,
    ];

    /// Its name in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Widget::Weather => "weather",
            Widget::Media => "media",
            Widget::System => "system",
            Widget::Start => "start",
            Widget::Apps => "apps",
            Widget::Claude => "claude",
            Widget::Codex => "codex",
            Widget::Controls => "controls",
            Widget::Battery => "battery",
            Widget::Tray => "tray",
            Widget::Lang => "lang",
            Widget::Desktop => "desktop",
            Widget::Clock => "clock",
            Widget::Mic => "mic",
            Widget::Network => "network",
            Widget::Theme => "theme",
            Widget::Progress => "progress",
            Widget::Antigravity => "antigravity",
            Widget::DeepSeek => "deepseek",
            Widget::Cursor => "cursor",
            Widget::Gmail => "gmail",
            Widget::Notifications => "notifications",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Widget::Weather => "Weather",
            Widget::Media => "Music",
            Widget::System => "CPU and RAM",
            Widget::Start => "Start button",
            Widget::Apps => "Apps",
            Widget::Claude => "Claude limits",
            Widget::Codex => "Codex limits",
            Widget::Controls => "Quick controls",
            Widget::Battery => "Battery",
            Widget::Tray => "Tray icons",
            Widget::Lang => "Input language",
            Widget::Desktop => "Show desktop",
            Widget::Clock => "Clock",
            Widget::Mic => "Mute and deafen",
            Widget::Network => "Network speed",
            Widget::Theme => "Light and dark",
            Widget::Progress => "Day, month, year",
            Widget::Antigravity => "Antigravity limits",
            Widget::DeepSeek => "DeepSeek balance",
            Widget::Cursor => "Cursor usage",
            Widget::Gmail => "Gmail unread",
            Widget::Notifications => "Notifications",
        }
    }

    /// The group a tile joins when it is switched on: 0 left, 1 centre, 2 right (of `GROUPS`).
    pub fn home_group(self) -> usize {
        match self {
            Widget::Weather | Widget::Media | Widget::System | Widget::Network => 0,
            Widget::Start | Widget::Apps => 1,
            _ => 2,
        }
    }

    pub fn parse(id: &str) -> Option<Widget> {
        Widget::ALL.into_iter().find(|w| w.id() == id)
    }

    /// The looks a tile comes in, the default first. Empty for tiles with only one.
    pub fn variants(self) -> &'static [Variant] {
        use Variant::*;
        match self {
            Widget::Clock => &[Large, Small, TearOff],
            _ => &[],
        }
    }
}

/// One of the looks a tile comes in, picked per tile in Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    ExtraLarge,
    Large,
    Medium,
    Small,
    TearOff,
}

impl Variant {
    pub const ALL: [Variant; 5] = [
        Variant::ExtraLarge,
        Variant::Large,
        Variant::Medium,
        Variant::Small,
        Variant::TearOff,
    ];
    pub fn id(self) -> &'static str {
        ["xl", "l", "m", "s", "tearoff"][self as usize]
    }
    pub fn label(self) -> &'static str {
        ["Extra large", "Large", "Medium", "Small", "Tear-off"][self as usize]
    }
}

/// The dock's tile groups: left, centre and right.
pub const GROUPS: usize = 3;

/// One entry of the dock's tile list: a tile, or the start of the next group (`|` in the file).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Slot {
    Tile(Widget),
    Break,
}

/// Buttons the quick controls tile can show, in the order they appear.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Control {
    Wifi,
    Bluetooth,
    Vpn,
    Airplane,
    Hotspot,
    Volume,
    Battery,
}

impl Control {
    pub const ALL: [Control; 7] = [
        Control::Wifi,
        Control::Bluetooth,
        Control::Vpn,
        Control::Airplane,
        Control::Hotspot,
        Control::Volume,
        Control::Battery,
    ];
    pub fn id(self) -> &'static str {
        [
            "wifi",
            "bluetooth",
            "vpn",
            "airplane",
            "hotspot",
            "volume",
            "battery",
        ][self as usize]
    }
    pub fn label(self) -> &'static str {
        [
            "Wi-Fi",
            "Bluetooth",
            "VPN",
            "Airplane",
            "Hotspot",
            "Volume",
            "Battery",
        ][self as usize]
    }
}

#[derive(Clone)]
pub struct Settings {
    pub style: Style,
    pub size: Size,
    pub top: bool,
    pub full: bool,
    pub every_monitor: bool, // a bar on every monitor, not just the primary one
    pub accent: usize,
    pub clock24: bool,
    pub hide_taskbar: bool,
    pub widgets: Vec<Slot>,
    /// The look picked for each tile that has a choice. Tiles not listed use their first.
    pub variants: Vec<(Widget, Variant)>,
    pub controls: Vec<Control>, // in Control::ALL order
    pub app_order: Vec<String>, // dock order the user dragged apps into, by app key
    /// Dock pins: shortcut paths, exe paths or `shell:AppsFolder\\<id>`.
    pub pins: Vec<String>,
    pub city: String,
    pub calendar: String,
    /// OAuth client from the user's own Google Cloud project. Google treats a desktop app's
    /// secret as not confidential, so it lives here; the refresh token is kept encrypted elsewhere.
    pub google_id: String,
    pub google_secret: String,
    /// DeepSeek API key, encrypted with DPAPI for this Windows account and base64'd.
    pub deepseek_key: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub fahrenheit: bool,
    pub auto_update: bool, // install a downloaded release when the user is away
}

impl Default for Settings {
    fn default() -> Self {
        use Widget::*;
        let tiles = |w: &[Widget]| w.iter().map(|&w| Slot::Tile(w)).collect::<Vec<_>>();
        let widgets = [
            tiles(&[Weather, Media]),
            tiles(&[Start, Apps]),
            tiles(&[Claude, Codex, Tray, Controls, Lang, Clock, Desktop]),
        ]
        .join(&Slot::Break);
        Settings {
            style: Style::Lagoon,
            size: Size::M,
            top: false,
            full: false,
            every_monitor: false,
            accent: 0,
            clock24: true,
            hide_taskbar: false,
            widgets,
            variants: vec![],
            controls: vec![
                Control::Wifi,
                Control::Bluetooth,
                Control::Volume,
                Control::Battery,
            ],
            app_order: vec![],
            pins: vec![],
            city: String::new(),
            calendar: String::new(),
            google_id: String::new(),
            google_secret: String::new(),
            deepseek_key: String::new(),
            lat: None,
            lon: None,
            fahrenheit: false,
            auto_update: true,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into()))
            .join("serenebar")
            .join("settings.ini")
    }

    /// `first_pins` gives the pins for a file that has never saved any (the first run).
    pub fn parse(txt: &str, first_pins: impl FnOnce() -> Vec<String>) -> Self {
        let mut s = Self::default();
        let mut pins = None;
        let list = |v: &str, sep: char| {
            v.split(sep)
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        };
        for (k, v) in txt
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim(), v.trim()))
        {
            match k {
                "style" => s.style = Style::parse(v).unwrap_or(s.style),
                "size" => {
                    s.size = Size::ALL
                        .into_iter()
                        .find(|z| z.id() == v)
                        .unwrap_or(s.size)
                }
                "edge" => s.top = v == "top",
                "width" => s.full = v == "full",
                "monitors" => s.every_monitor = v == "all",
                "accent" => {
                    s.accent = ACCENTS
                        .iter()
                        .position(|a| a.1.eq_ignore_ascii_case(v))
                        .unwrap_or(0)
                }
                "clock" => s.clock24 = v != "12",
                "taskbar" => s.hide_taskbar = v == "hidden",
                "widgets" => {
                    s.widgets = vec![];
                    for w in list(v, ',') {
                        // the tear-off clock was its own tile before clocks had looks
                        let slot = match w.as_str() {
                            "|" => Some(Slot::Break),
                            "dateclock" => {
                                s.variants.push((Widget::Clock, Variant::TearOff));
                                Some(Slot::Tile(Widget::Clock))
                            }
                            _ => Widget::parse(&w).map(Slot::Tile),
                        };
                        if let Some(slot) =
                            slot.filter(|t| *t == Slot::Break || !s.widgets.contains(t))
                        {
                            s.widgets.push(slot);
                        }
                    }
                }
                "variants" => {
                    s.variants = list(v, ',')
                        .iter()
                        .filter_map(|p| {
                            let (w, x) = p.split_once(':')?;
                            let (w, x) = (
                                Widget::parse(w)?,
                                Variant::ALL.into_iter().find(|x2| x2.id() == x)?,
                            );
                            w.variants().contains(&x).then_some((w, x))
                        })
                        .collect()
                }
                "controls" => {
                    s.controls = Control::ALL
                        .into_iter()
                        .filter(|c| list(v, ',').iter().any(|x| x == c.id()))
                        .collect()
                }
                // app keys are exe paths or AppUserModelIDs; neither can contain '|'
                "app_order" => s.app_order = list(v, '|'),
                "pins" => pins = Some(list(v, '|')),
                "city" => s.city = v.to_string(),
                "calendar" => s.calendar = v.to_string(),
                "google_client_id" => s.google_id = v.to_string(),
                "google_client_secret" => s.google_secret = v.to_string(),
                "deepseek_key" => s.deepseek_key = v.to_string(),
                "latitude" => s.lat = v.parse().ok(),
                "longitude" => s.lon = v.parse().ok(),
                "units" => s.fahrenheit = v == "f",
                "updates" => s.auto_update = v != "manual",
                _ => {}
            }
        }
        s.pins = pins.unwrap_or_else(first_pins);
        s
    }

    pub fn to_ini(&self) -> String {
        let opt = |o: Option<f64>| o.map(|v| v.to_string()).unwrap_or_default();
        let widgets: Vec<&str> = self
            .widgets
            .iter()
            .map(|s| match s {
                Slot::Tile(w) => w.id(),
                Slot::Break => "|",
            })
            .collect();
        let controls: Vec<&str> = self.controls.iter().map(|c| c.id()).collect();
        let variants: Vec<String> = self
            .variants
            .iter()
            .map(|(w, x)| format!("{}:{}", w.id(), x.id()))
            .collect();
        format!(
            "style={}\nsize={}\nedge={}\nwidth={}\nmonitors={}\naccent={}\nclock={}\ntaskbar={}\nupdates={}\nwidgets={}\nvariants={}\ncontrols={}\napp_order={}\npins={}\n\
             # weather: a city name, or latitude/longitude in decimal degrees to override it. units is c or f.\n\
             city={}\nlatitude={}\nlongitude={}\nunits={}\n\
             # calendar: the secret iCal (.ics) link from Google, Outlook or iCloud\ncalendar={}\n\
             # Google Calendar sign-in: an OAuth client (Desktop app) from your Google Cloud project\n\
             google_client_id={}\ngoogle_client_secret={}\n\
             # DeepSeek API key, encrypted so only this Windows account can read it. Set it in Settings.\n\
             deepseek_key={}\n",
            self.style.name(), self.size.id(), if self.top { "top" } else { "bottom" },
            if self.full { "full" } else { "pill" }, if self.every_monitor { "all" } else { "primary" }, ACCENTS[self.accent].1.to_lowercase(),
            if self.clock24 { 24 } else { 12 }, if self.hide_taskbar { "hidden" } else { "autohide" },
            if self.auto_update { "auto" } else { "manual" },
            widgets.join(","), variants.join(","), controls.join(","), self.app_order.join("|"), self.pins.join("|"), self.city, opt(self.lat), opt(self.lon),
            if self.fahrenheit { "f" } else { "c" }, self.calendar, self.google_id, self.google_secret,
            self.deepseek_key,
        )
    }

    pub fn load(first_pins: impl FnOnce() -> Vec<String>) -> Self {
        // lossy, so a file Notepad saved in the ANSI code page still keeps its other settings
        let txt = std::fs::read(Self::path())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        Self::parse(txt.trim_start_matches('\u{feff}'), first_pins) // Notepad's "UTF-8 with BOM"
    }

    pub fn save(&self) {
        let p = Self::path();
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, self.to_ini());
    }

    pub fn shows(&self, w: Widget) -> bool {
        self.widgets.contains(&Slot::Tile(w))
    }

    /// The look of tile `w`: the one picked, else its first.
    pub fn variant(&self, w: Widget) -> Variant {
        self.variants
            .iter()
            .find(|v| v.0 == w)
            .map(|v| v.1)
            .or(w.variants().first().copied())
            .unwrap_or(Variant::Large)
    }

    /// Moves tile `w` on to its next look, round to the first after the last.
    pub fn next_variant(&mut self, w: Widget) {
        let all = w.variants();
        let Some(i) = all.iter().position(|&x| x == self.variant(w)) else {
            return;
        };
        let next = all[(i + 1) % all.len()];
        self.variants.retain(|v| v.0 != w);
        if next != all[0] {
            self.variants.push((w, next));
        }
    }
}

/// A tile as the settings window lists it: switched on or not, and which group it's in.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetRow {
    pub widget: Widget,
    pub on: bool,
    pub group: usize,
}

/// The tile list as rows for the settings window: enabled tiles in order, then the rest.
pub fn widget_rows(slots: &[Slot]) -> Vec<WidgetRow> {
    let mut rows: Vec<WidgetRow> = vec![];
    let mut group = 0;
    for s in slots {
        match *s {
            Slot::Break => group = (group + 1).min(GROUPS - 1),
            Slot::Tile(w) if !rows.iter().any(|r| r.widget == w) => rows.push(WidgetRow {
                widget: w,
                on: true,
                group,
            }),
            Slot::Tile(_) => {}
        }
    }
    for w in Widget::ALL {
        if !rows.iter().any(|r| r.widget == w) {
            rows.push(WidgetRow {
                widget: w,
                on: false,
                group: w.home_group(),
            });
        }
    }
    rows
}

/// Moves row `from` into `group`, before the `k`-th other shown row of that group (or to its
/// end). Only the order of shown rows within a group matters, so the rest keep their order.
pub fn move_row(rows: &mut Vec<WidgetRow>, from: usize, group: usize, k: usize) {
    let mut r = rows.remove(from);
    r.group = group;
    let members: Vec<usize> = (0..rows.len())
        .filter(|&i| rows[i].on && rows[i].group == group)
        .collect();
    let at = match (members.get(k), members.last()) {
        (Some(&i), _) => i,
        (None, Some(&last)) => last + 1,
        (None, None) => rows.len(),
    };
    rows.insert(at, r);
}

pub fn widgets_from_rows(rows: &[WidgetRow]) -> Vec<Slot> {
    let mut out = vec![];
    for g in 0..GROUPS {
        if g > 0 {
            out.push(Slot::Break);
        }
        out.extend(
            rows.iter()
                .filter(|r| r.on && r.group == g)
                .map(|r| Slot::Tile(r.widget)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Widget::*;

    fn contrast(a: u32, b: u32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn accent_colours_are_readable() {
        // pal() in ui.rs: panel background and card, light then dark
        let (light, dark) = ([0xF2F2F7, 0xFFFFFF], [0x1C1C1E, 0x2C2C2E]);
        for (i, &(fill, name)) in ACCENTS.iter().enumerate() {
            // labels on a fill are bold, so 3:1 is enough
            let fg = if dark_text_on(fill) {
                0x1D1D1F
            } else {
                0xFFFFFF
            };
            assert!(contrast(fg, fill) >= 3.0, "text on {name}");
            let (l, d) = ACCENT_TEXT[i];
            for bg in light {
                assert!(contrast(l, bg) >= 4.5, "{name} text on {bg:06X}");
            }
            for bg in dark {
                assert!(contrast(d, bg) >= 4.5, "{name} text on {bg:06X}");
            }
        }
        assert!(dark_text_on(0x30D158)); // the charging battery chip
        assert!(!dark_text_on(0x0A84FF));
    }

    fn parse_test(txt: &str) -> Settings {
        Settings::parse(txt, Vec::new)
    }

    #[test]
    fn settings_roundtrip() {
        let s = parse_test("style=dusk\nsize=l\nedge=top\nwidth=full\nmonitors=all\naccent=green\nclock=12\ntaskbar=hidden\nwidgets=clock,|,apps,nope\ncity=Berlin\ncalendar=webcal://x/y.ics\nlatitude=52.5\nlongitude=13.4\nunits=f\n");
        assert!(
            s.style == Style::Dusk
                && s.size == Size::L
                && s.top
                && s.full
                && s.accent == 6
                && !s.clock24
                && s.hide_taskbar
                && s.fahrenheit
        );
        assert_eq!(
            s.widgets,
            [Slot::Tile(Clock), Slot::Break, Slot::Tile(Apps)]
        ); // unknown ids dropped
        let t = parse_test(&s.to_ini());
        assert!(
            t.style == s.style
                && t.size == s.size
                && t.top
                && t.full
                && t.every_monitor
                && t.accent == 6
                && t.lat == Some(52.5)
                && t.widgets == s.widgets
        );
        assert!(t.city == "Berlin" && t.calendar == "webcal://x/y.ics");
        assert!(Settings::default().auto_update && t.auto_update);
        assert!(!parse_test(&parse_test("updates=manual").to_ini()).auto_update);
        let g = parse_test(
            "google_client_id=123.apps.googleusercontent.com\ngoogle_client_secret=GOCSPX-x",
        );
        let g2 = parse_test(&g.to_ini());
        assert_eq!(
            (g2.google_id.as_str(), g2.google_secret.as_str()),
            ("123.apps.googleusercontent.com", "GOCSPX-x")
        );
        let k = parse_test("deepseek_key=AQAAANCMnd8B+/=");
        assert_eq!(parse_test(&k.to_ini()).deepseek_key, "AQAAANCMnd8B+/=");
        let d = parse_test("garbage\nstyle=nope");
        assert!(d.style == Style::Lagoon && d.size == Size::M);
        assert_eq!(
            parse_test(&Settings::default().to_ini()).widgets,
            Settings::default().widgets
        );
    }

    #[test]
    fn widget_rows_roundtrip() {
        let d = Settings::default().widgets;
        let rows = widget_rows(&d);
        assert_eq!(rows.len(), Widget::ALL.len());
        assert_eq!(widgets_from_rows(&rows), d);
        let apps = rows.iter().find(|r| r.widget == Apps).unwrap();
        assert!(apps.on && apps.group == 1);
        let system = rows.iter().find(|r| r.widget == System).unwrap();
        assert!(!system.on && system.group == System.home_group());
        // a repeated tile shows once, and more than three groups fold into the last
        let rows = widget_rows(&[
            Slot::Tile(Media),
            Slot::Tile(Media),
            Slot::Break,
            Slot::Break,
            Slot::Break,
            Slot::Tile(Clock),
        ]);
        assert_eq!(
            widgets_from_rows(&rows),
            [
                Slot::Tile(Media),
                Slot::Break,
                Slot::Break,
                Slot::Tile(Clock)
            ]
        );
    }

    #[test]
    fn dragging_rows_between_groups() {
        let row = |widget, group| WidgetRow {
            widget,
            on: true,
            group,
        };
        let mut rows = vec![
            row(Weather, 0),
            row(Media, 0),
            row(Claude, 2),
            row(Clock, 2),
        ];
        let ids = |rows: &[WidgetRow]| {
            widgets_from_rows(rows)
                .iter()
                .map(|s| match s {
                    Slot::Tile(w) => w.id(),
                    Slot::Break => "|",
                })
                .collect::<Vec<_>>()
        };
        move_row(&mut rows, 0, 2, 1); // weather between claude and clock
        assert_eq!(
            ids(&rows),
            ["media", "|", "|", "claude", "weather", "clock"]
        );
        move_row(&mut rows, 0, 1, 0); // media into the empty centre group
        assert_eq!(
            ids(&rows),
            ["|", "media", "|", "claude", "weather", "clock"]
        );
        let last = rows.iter().position(|r| r.widget == Clock).unwrap();
        move_row(&mut rows, last, 2, 0); // clock to the top of its own group
        assert_eq!(
            ids(&rows),
            ["|", "media", "|", "clock", "claude", "weather"]
        );
        // hidden rows don't count: adding one puts it after the group's last shown row
        rows.push(WidgetRow {
            widget: Battery,
            on: false,
            group: 0,
        });
        let i = rows.iter().position(|r| r.widget == Claude).unwrap();
        rows[i].on = false;
        let b = rows.len() - 1;
        rows[b].on = true;
        move_row(&mut rows, b, 2, usize::MAX);
        assert_eq!(
            ids(&rows),
            ["|", "media", "|", "clock", "weather", "battery"]
        );
    }

    #[test]
    fn controls_and_app_order_roundtrip() {
        let s = parse_test(
            "controls=battery, vpn,nope,volume\napp_order=c:\\a.exe|microsoft.windows.explorer",
        );
        assert_eq!(
            s.controls,
            [Control::Vpn, Control::Volume, Control::Battery]
        ); // kept in tile order, unknowns dropped
        assert_eq!(s.app_order, [r"c:\a.exe", "microsoft.windows.explorer"]);
        let t = parse_test(&s.to_ini());
        assert_eq!((t.controls, t.app_order), (s.controls, s.app_order));
    }

    #[test]
    fn pins_roundtrip_and_first_run() {
        let first = || vec!["taskbar.lnk".to_string()];
        assert_eq!(Settings::parse("style=dusk", first).pins, ["taskbar.lnk"]); // never saved: the taskbar's
        assert!(Settings::parse("pins=", first).pins.is_empty()); // the user unpinned everything
        let s = parse_test(
            r"pins=C:\x\Discord.lnk|shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App|c:\apps\zed.exe",
        );
        assert_eq!(s.pins.len(), 3);
        assert_eq!(parse_test(&s.to_ini()).pins, s.pins);
    }

    #[test]
    fn variants_roundtrip_and_old_tear_off_clock() {
        let old = parse_test("widgets=clock,dateclock,|,apps");
        assert_eq!(
            old.widgets,
            [Slot::Tile(Clock), Slot::Break, Slot::Tile(Apps)]
        );
        assert_eq!(old.variant(Clock), Variant::TearOff);
        let mut s = parse_test("variants=clock:s,apps:xl,nope:l,clock:zz");
        assert_eq!(s.variants, [(Clock, Variant::Small)]); // apps has no looks
        assert_eq!(parse_test(&s.to_ini()).variant(Clock), Variant::Small);
        assert_eq!(s.variant(Media), Variant::Large);
        s.next_variant(Clock);
        assert_eq!(s.variant(Clock), Variant::TearOff);
        s.next_variant(Clock);
        assert!(s.variant(Clock) == Variant::Large && s.variants.is_empty());
    }
}
