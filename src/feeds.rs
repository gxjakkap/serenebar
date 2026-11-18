//! Background fetching: weather, calendar events, AI plan limits and the update check.
use crate::calendar::Event;
use crate::calendar::{fetch_ics, to_local};
use crate::http::curl;
use crate::limits::Limit;
use crate::limits::{antigravity_limits, claude_limits, codex_limits};
use crate::weather::Weather;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;

/// What the feeds last fetched. Read it with `snapshot`.
#[derive(Clone, Default)]
pub struct State {
    pub weather: Option<Weather>,
    pub events: Vec<Event>,
    pub claude: Option<Vec<Limit>>,
    pub codex: Option<Vec<Limit>>,
    pub antigravity: Option<Vec<Limit>>,
    pub update: Option<crate::update::Release>,
    /// That release's installer, downloaded and checked.
    pub installer: Option<std::path::PathBuf>,
    /// How a check from Settings went: "Checking…", "Up to date" and so on, empty before one.
    pub update_note: &'static str,
}

static STATE: Mutex<State> = Mutex::new(State {
    weather: None,
    events: vec![],
    claude: None,
    codex: None,
    antigravity: None,
    update: None,
    installer: None,
    update_note: "",
});

pub fn snapshot() -> State {
    STATE.lock().unwrap().clone()
}

#[derive(Clone, PartialEq)]
pub struct FeedConfig {
    pub city: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub fahrenheit: bool,
    pub calendar: String,
    pub google_id: String,
    pub google_secret: String,
}

pub enum Msg {
    /// Where to get things from. The worker starts fetching once it has the first one.
    Config(FeedConfig),
    /// The Google account changed: drop the events and fetch everything again now.
    Refetch,
    /// Settings asked: look for a newer release now.
    CheckUpdate,
}

/// Weather and calendar every 15 minutes, Claude and Antigravity limits every 5, Codex limits every minute,
/// releases once a day.
pub fn spawn_feeds(hwnd: HWND) -> Sender<Msg> {
    let h = hwnd.0 as isize;
    let (tx, rx) = channel::<Msg>();
    std::thread::spawn(move || {
        let mut cfg = loop {
            match rx.recv() {
                Ok(Msg::Config(c)) => break c,
                Ok(_) => {}
                Err(_) => return,
            }
        };
        let fixed = |c: &FeedConfig| match (c.lat, c.lon) {
            (Some(lat), Some(lon)) => Some((lat, lon, c.city.clone())),
            _ => None,
        };
        let mut place = fixed(&cfg);
        let mut minute = 0u64;
        let mut next_release_check = Instant::now();
        let mut asked = false; // a check from Settings, which wants to hear how it went
        loop {
            let mut changed = false;
            if minute.is_multiple_of(15) {
                if place.is_none() && !cfg.city.is_empty() {
                    place = curl(&crate::weather::geocode_url(&cfg.city, 1), None)
                        .and_then(|b| crate::weather::parse_places(&b).into_iter().next())
                        .map(|p| (p.lat, p.lon, p.name));
                }
                if let Some(w) = place.as_ref().and_then(|(lat, lon, city)| {
                    curl(
                        &crate::weather::weather_url(*lat, *lon, cfg.fahrenheit),
                        None,
                    )
                    .and_then(|b| crate::weather::parse_weather(&b, city))
                }) {
                    STATE.lock().unwrap().weather = Some(w);
                    changed = true;
                }
                // iCal link and Google account, merged
                let t = crate::system::local_time().days();
                let (from, to) = (t + crate::calendar::WINDOW.0, t + crate::calendar::WINDOW.1);
                let ics = (!cfg.calendar.is_empty())
                    .then(|| fetch_ics(&cfg.calendar, from, to))
                    .flatten();
                let google = crate::google::events(&cfg.google_id, &cfg.google_secret, from, to);
                changed |= google.is_none() && !cfg.google_id.is_empty(); // so Settings shows why
                if ics.is_some() || google.is_some() {
                    let mut ev: Vec<Event> = ics
                        .into_iter()
                        .chain(google)
                        .flatten()
                        .map(|e| Event {
                            at: to_local(e.at),
                            ..e
                        })
                        .collect();
                    ev.sort_by_key(|e| (e.at.day, e.at.sec.unwrap_or(-1)));
                    STATE.lock().unwrap().events = ev;
                    changed = true;
                }
            }
            if minute.is_multiple_of(5) {
                if let Some(l) = claude_limits() {
                    STATE.lock().unwrap().claude = Some(l);
                    changed = true;
                }
                if let Some(l) = antigravity_limits() {
                    STATE.lock().unwrap().antigravity = Some(l);
                    changed = true;
                }
            }
            // its own clock, since a settings change restarts `minute`
            if Instant::now() >= next_release_check {
                let found = crate::update::latest();
                // a failed check (offline at login, say) tries again in an hour
                let wait = if found.is_some() { 24 } else { 1 };
                next_release_check = Instant::now() + Duration::from_secs(wait * 3600);
                let newer = found.as_ref().is_some_and(crate::update::is_update);
                if asked {
                    STATE.lock().unwrap().update_note = match (&found, newer) {
                        (None, _) => "Couldn't reach GitHub",
                        (Some(_), false) => "serenebar is up to date",
                        (Some(_), true) => "",
                    };
                    asked = false;
                    changed = true;
                }
                if let Some(r) = found.filter(|_| newer) {
                    let mut st = STATE.lock().unwrap();
                    if st.update.as_ref() != Some(&r) {
                        (st.update, st.installer) = (Some(r.clone()), None);
                        changed = true;
                    }
                    let have = st.installer.is_some();
                    drop(st);
                    // a failed download tries again at the next check
                    if !have {
                        let path = crate::update::download(&r);
                        changed |= path.is_some();
                        STATE.lock().unwrap().installer = path;
                    }
                }
            }
            if let Some(l) = codex_limits() {
                STATE.lock().unwrap().codex = Some(l);
                changed = true;
            }
            if changed {
                crate::system::notify(h);
            }
            minute += 1;
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(Msg::Config(new)) => {
                    let mut st = STATE.lock().unwrap();
                    if (&new.city, new.lat, new.lon) != (&cfg.city, cfg.lat, cfg.lon) {
                        st.weather = None;
                    }
                    if (&new.calendar, &new.google_id, &new.google_secret)
                        != (&cfg.calendar, &cfg.google_id, &cfg.google_secret)
                    {
                        st.events.clear();
                    }
                    drop(st);
                    cfg = new;
                    place = fixed(&cfg);
                    minute = 0;
                }
                Ok(Msg::Refetch) => {
                    STATE.lock().unwrap().events.clear();
                    minute = 0;
                }
                Ok(Msg::CheckUpdate) => {
                    STATE.lock().unwrap().update_note = "Checking…";
                    crate::system::notify(h);
                    next_release_check = Instant::now();
                    asked = true;
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    });
    tx
}
