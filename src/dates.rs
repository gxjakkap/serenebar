//! Calendar dates as days since 1970, and the few formats the feeds use.

/// A local date and time, as the clock tiles show it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalTime {
    /// Days since 1970-01-01.
    pub fn days(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let (m, d) = (m as i64, d as i64);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y as i32, m, d)
}

/// 0 is Sunday.
pub fn weekday(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "Mon" for weekday 1.
pub fn day_name(weekday: u32) -> &'static str {
    WEEKDAYS[weekday as usize % 7]
}

/// "Monday" for weekday 1.
pub fn day_full_name(weekday: u32) -> &'static str {
    [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ][weekday as usize % 7]
}

/// "November" for month 11.
pub fn month_name(month: u32) -> &'static str {
    MONTHS[(month as usize + 11) % 12]
}

/// "Nov" for month 11.
pub fn month_abbr(month: u32) -> &'static str {
    &month_name(month)[..3]
}

/// The month `delta` months after (year, month).
pub fn add_month((y, m): (i32, u32), delta: i32) -> (i32, u32) {
    let n = y * 12 + m as i32 - 1 + delta;
    (n.div_euclid(12), n.rem_euclid(12) as u32 + 1)
}

/// Days since 1970 from "2026-11-15".
pub fn parse_date(s: &str) -> Option<i64> {
    iso_to_unix(s.get(..10)?).map(|t| t.div_euclid(86400))
}

/// 1 to 12.
pub fn hour12(hour: u32) -> u32 {
    (hour + 11) % 12 + 1
}

/// "15:04", or "3:04 PM" on a 12-hour clock.
pub fn fmt_hm(hour: u32, minute: u32, h24: bool) -> String {
    if h24 {
        format!("{hour}:{minute:02}")
    } else {
        format!(
            "{}:{minute:02} {}",
            hour12(hour),
            if hour < 12 { "AM" } else { "PM" }
        )
    }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Unix seconds from "2026-11-15T19:00:00.25+02:00", "...Z" or a bare date.
pub fn iso_to_unix(s: &str) -> Option<i64> {
    let num = |a: usize, n: usize| s.get(a..a + n)?.parse::<i64>().ok();
    let day = days_from_civil(num(0, 4)? as i32, num(5, 2)? as u32, num(8, 2)? as u32);
    let (h, mi, sec) = if s.len() >= 19 {
        (num(11, 2)?, num(14, 2)?, num(17, 2)?)
    } else {
        (0, 0, 0)
    };
    let rest = s
        .get(19..)
        .unwrap_or("")
        .trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let off = match rest.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(0);
            (oh * 3600 + om * 60) * if sign == '-' { -1 } else { 1 }
        }
        _ => 0,
    };
    Some(day * 86400 + h * 3600 + mi * 60 + sec - off)
}

/// "2026-11-15T19:00:00Z" from unix seconds.
pub fn unix_to_iso(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86400));
    let s = t.rem_euclid(86400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// "3d 4h", "2h 14m", "12m".
pub fn fmt_left(secs: i64) -> String {
    let m = (secs.max(0) + 59) / 60;
    if m >= 1440 {
        format!("{}d {}h", m / 1440, m % 1440 / 60)
    } else if m >= 60 {
        format!("{}h {}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

/// How much of today, this month and this year has gone by, each from 0 to 1.
pub fn progress(t: LocalTime) -> [f32; 3] {
    let day = (t.hour * 3600 + t.minute * 60 + t.second) as f64 / 86400.0;
    let since = |y: i32, m: u32| (t.days() - days_from_civil(y, m, 1)) as f64 + day;
    let (ny, nm) = add_month((t.year, t.month), 1);
    let month_len = (days_from_civil(ny, nm, 1) - days_from_civil(t.year, t.month, 1)) as f64;
    let year_len = (days_from_civil(t.year + 1, 1, 1) - days_from_civil(t.year, 1, 1)) as f64;
    [
        day,
        since(t.year, t.month) / month_len,
        since(t.year, 1) / year_len,
    ]
    .map(|f| f as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_roundtrip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(
            civil_from_days(days_from_civil(2026, 2, 28) + 1),
            (2026, 3, 1)
        );
        assert_eq!(
            civil_from_days(days_from_civil(2028, 2, 28) + 1),
            (2028, 2, 29)
        );
        assert_eq!(weekday(days_from_civil(2026, 11, 15)), 0); // a Sunday
        for z in [-800_000, -1, 0, 1, 20_000, 400_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }

    #[test]
    fn iso_and_durations() {
        assert_eq!(iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_unix("1970-01-01T02:00:00.123456+02:00"), Some(0));
        assert_eq!(
            iso_to_unix("2026-11-15T19:00:00+00:00"),
            Some(days_from_civil(2026, 11, 15) * 86400 + 19 * 3600)
        );
        assert_eq!(fmt_left(2 * 3600 + 14 * 60), "2h 14m");
        assert_eq!(fmt_left(3 * 86400 + 4 * 3600), "3d 4h");
        assert_eq!(fmt_left(30), "1m");
        assert_eq!(unix_to_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            iso_to_unix(&unix_to_iso(1_763_900_000)),
            Some(1_763_900_000)
        );
    }

    #[test]
    fn names_and_months() {
        assert_eq!(
            (
                day_name(weekday(days_from_civil(2026, 11, 16))),
                month_name(11),
                month_abbr(1)
            ),
            ("Mon", "November", "Jan")
        );
        assert_eq!(add_month((2026, 12), 1), (2027, 1));
        assert_eq!(add_month((2026, 1), -1), (2025, 12));
        assert_eq!(
            parse_date("2026-11-20"),
            Some(days_from_civil(2026, 11, 20))
        );
        assert_eq!(
            parse_date("2026-11-20T23:00"),
            Some(days_from_civil(2026, 11, 20))
        );
        assert_eq!(
            (
                fmt_hm(15, 4, true),
                fmt_hm(15, 4, false),
                fmt_hm(0, 5, false),
                fmt_hm(12, 0, false)
            ),
            (
                "15:04".into(),
                "3:04 PM".into(),
                "12:05 AM".into(),
                "12:00 PM".into()
            )
        );
    }

    #[test]
    fn day_month_year_progress() {
        let at = |month, day, hour| LocalTime {
            year: 2026,
            month,
            day,
            hour,
            minute: 0,
            second: 0,
        };
        assert_eq!(progress(at(1, 1, 0)), [0.0, 0.0, 0.0]);
        assert_eq!(progress(at(1, 1, 12))[0], 0.5);
        let [_, m, y] = progress(at(2, 15, 0)); // 14 of February's 28 days gone
        assert_eq!(m, 0.5);
        assert!((y - 45.0 / 365.0).abs() < 1e-6);
        let leap = LocalTime {
            year: 2028,
            ..at(12, 31, 0)
        };
        assert!((progress(leap)[2] - 365.0 / 366.0).abs() < 1e-6);
    }
}
