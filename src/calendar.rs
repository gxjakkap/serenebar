//! Calendar events from an iCal feed and from Google Calendar.
use crate::dates::{days_from_civil, weekday};

/// Days around today the calendar fetches: a month and a half back, two and a half ahead.
pub const WINDOW: (i64, i64) = (-45, 75);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stamp {
    pub day: i64,         // days since 1970-01-01
    pub sec: Option<i64>, // seconds into the day, None for all-day events
    pub utc: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub at: Stamp,
    pub summary: String,
    pub location: String,
}

fn parse_stamp(v: &str) -> Option<Stamp> {
    let n = |a: usize, b: usize| v.get(a..b)?.parse::<i64>().ok();
    let day = days_from_civil(n(0, 4)? as i32, n(4, 6)? as u32, n(6, 8)? as u32);
    if v.len() < 15 {
        return Some(Stamp {
            day,
            sec: None,
            utc: false,
        });
    }
    Some(Stamp {
        day,
        sec: Some(n(9, 11)? * 3600 + n(11, 13)? * 60 + n(13, 15)?),
        utc: v.ends_with('Z'),
    })
}

fn unescape(s: &str) -> String {
    s.replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\\\", "\\")
}

/// Events of an iCal feed that fall between days `from` and `to`, sorted.
/// TZID times are read as local time.
pub fn parse_ics(text: &str, from: i64, to: i64) -> Vec<Event> {
    let mut lines: Vec<String> = vec![];
    for l in text.lines() {
        match (
            l.strip_prefix(' ').or_else(|| l.strip_prefix('\t')),
            lines.last_mut(),
        ) {
            (Some(cont), Some(prev)) => prev.push_str(cont), // folded line
            _ => lines.push(l.to_string()),
        }
    }
    let mut out = vec![];
    let (mut start, mut summary, mut loc, mut rrule, mut inside) =
        (None, String::new(), String::new(), String::new(), false);
    for l in &lines {
        let Some((head, val)) = l.split_once(':') else {
            continue;
        };
        let name = head.split(';').next().unwrap_or(head);
        match name {
            "BEGIN" if val == "VEVENT" => {
                inside = true;
                start = None;
                summary.clear();
                loc.clear();
                rrule.clear();
            }
            "END" if val == "VEVENT" && inside => {
                inside = false;
                if let Some(s) = start {
                    let (summary, location) = (unescape(&summary), unescape(&loc));
                    expand(s, &rrule, from, to, |at| {
                        out.push(Event {
                            at,
                            summary: summary.clone(),
                            location: location.clone(),
                        })
                    });
                }
            }
            "DTSTART" if inside => start = parse_stamp(val),
            "SUMMARY" if inside => summary = val.to_string(),
            "LOCATION" if inside => loc = val.to_string(),
            "RRULE" if inside => rrule = val.to_string(),
            _ => {}
        }
    }
    out.sort_by_key(|e| (e.at.day, e.at.sec.unwrap_or(-1)));
    out
}

/// ponytail: repeats DAILY and WEEKLY rules (with INTERVAL, COUNT, UNTIL, BYDAY). EXDATE and
/// MONTHLY/YEARLY rules aren't expanded, so those show only their first occurrence.
fn expand(s: Stamp, rrule: &str, from: i64, to: i64, mut emit: impl FnMut(Stamp)) {
    let get = |k: &str| {
        rrule
            .split(';')
            .find_map(|p| p.strip_prefix(k)?.strip_prefix('='))
    };
    let interval = get("INTERVAL")
        .and_then(|x| x.parse::<i64>().ok())
        .unwrap_or(1)
        .max(1);
    let count = get("COUNT")
        .and_then(|x| x.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let until = get("UNTIL")
        .and_then(parse_stamp)
        .map_or(i64::MAX, |u| u.day);
    let mut n = 0usize;
    let mut take = |day: i64| -> bool {
        if day > until || day > to || n >= count {
            return false;
        }
        n += 1;
        if day >= from {
            emit(Stamp { day, ..s });
        }
        true
    };
    match get("FREQ") {
        Some("DAILY") => {
            let mut d = s.day;
            while take(d) {
                d += interval;
            }
        }
        Some("WEEKLY") => {
            const CODES: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];
            let mut by: Vec<i64> = get("BYDAY")
                .map(|b| {
                    b.split(',')
                        .filter_map(|x| CODES.iter().position(|c| x.ends_with(c)).map(|i| i as i64))
                        .collect()
                })
                .unwrap_or_default();
            if by.is_empty() {
                by.push(weekday(s.day) as i64);
            }
            by.sort();
            let mut week = s.day - weekday(s.day) as i64;
            'weeks: loop {
                for &wd in &by {
                    let d = week + wd;
                    if d < s.day {
                        continue;
                    }
                    if !take(d) {
                        break 'weeks;
                    }
                }
                week += 7 * interval;
            }
        }
        _ => {
            take(s.day);
        }
    }
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::*;
    use crate::http::curl;
    use windows::Win32::Foundation::*;

    /// Events of an iCal feed between days `from` and `to`, times as the feed gives them.
    pub fn fetch_ics(url: &str, from: i64, to: i64) -> Option<Vec<Event>> {
        let url = url.replacen("webcal://", "https://", 1);
        Some(parse_ics(&curl(&url, None)?, from, to))
    }

    pub fn to_local(s: Stamp) -> Stamp {
        use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
        let (Some(sec), true) = (s.sec, s.utc) else {
            return s;
        };
        let (y, m, d) = crate::dates::civil_from_days(s.day);
        let utc = SYSTEMTIME {
            wYear: y as u16,
            wMonth: m as u16,
            wDay: d as u16,
            wHour: (sec / 3600) as u16,
            wMinute: (sec / 60 % 60) as u16,
            wSecond: (sec % 60) as u16,
            ..Default::default()
        };
        let mut local = SYSTEMTIME::default();
        if unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_err() {
            return s;
        }
        Stamp {
            day: crate::dates::days_from_civil(
                local.wYear as i32,
                local.wMonth as u32,
                local.wDay as u32,
            ),
            sec: Some(local.wHour as i64 * 3600 + local.wMinute as i64 * 60),
            utc: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dates::civil_from_days;

    #[test]
    fn ics_with_weekly_repeat() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261102T090000Z\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=5\r\nSUMMARY:Standup\\, team\r\nLOCATION:Zo\r\n om\r\nEND:VEVENT\r\n\
                   BEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261120\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let from = days_from_civil(2026, 11, 1);
        let ev = parse_ics(ics, from, from + 60);
        let days: Vec<(i32, u32, u32)> = ev.iter().map(|e| civil_from_days(e.at.day)).collect();
        assert_eq!(
            days,
            vec![
                (2026, 11, 2),
                (2026, 11, 4),
                (2026, 11, 9),
                (2026, 11, 11),
                (2026, 11, 16),
                (2026, 11, 20)
            ]
        );
        assert_eq!(
            (
                ev[0].summary.as_str(),
                ev[0].location.as_str(),
                ev[0].at.sec,
                ev[0].at.utc
            ),
            ("Standup, team", "Zoom", Some(9 * 3600), true)
        );
        assert_eq!(ev[5].at.sec, None);
        // a window that starts mid-series still counts earlier occurrences against COUNT
        assert_eq!(
            parse_ics(ics, days_from_civil(2026, 11, 12), from + 60).len(),
            2
        );
    }
}
