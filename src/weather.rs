//! Weather from Open-Meteo, which needs no API key.
use crate::dates::{parse_date, weekday};
use crate::http::pct;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub city: String,
    pub temp: f32,
    pub code: u32,
    pub day: bool,
    pub hourly: Vec<Hour>,
    pub daily: Vec<Day>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hour {
    pub hour: u32,
    pub code: u32,
    pub temp: f32,
    pub day: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub weekday: u32,
    pub code: u32,
    pub min: f32,
    pub max: f32,
}

/// A place the geocoder knows. `area` is what tells same-named places apart: "Texas, United States".
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub area: String,
    pub lat: f64,
    pub lon: f64,
}

/// Up to `count` places matching `city`, best match first.
pub fn geocode_url(city: &str, count: usize) -> String {
    format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count={count}",
        pct(city)
    )
}

pub fn parse_places(body: &str) -> Vec<Place> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return vec![];
    };
    let Some(results) = v["results"].as_array() else {
        return vec![];
    };
    results
        .iter()
        .filter_map(|r| {
            let name = r["name"].as_str()?.to_string();
            let parts = [r["admin1"].as_str(), r["country"].as_str()];
            let area = parts
                .into_iter()
                .flatten()
                .filter(|p| !p.is_empty() && *p != name)
                .collect::<Vec<_>>()
                .join(", ");
            Some(Place {
                area,
                lat: r["latitude"].as_f64()?,
                lon: r["longitude"].as_f64()?,
                name,
            })
        })
        .collect()
}

pub fn weather_url(lat: f64, lon: f64, fahrenheit: bool) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&timezone=auto&forecast_days=7&forecast_hours=24\
         &current=temperature_2m,weather_code,is_day&hourly=temperature_2m,weather_code,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min{}",
        if fahrenheit { "&temperature_unit=fahrenheit" } else { "" }
    )
}

pub fn parse_weather(body: &str, city: &str) -> Option<Weather> {
    let v: Value = serde_json::from_str(body).ok()?;
    let c = &v["current"];
    let f = |x: &Value| x.as_f64().map(|n| n as f32);
    let arr = |sec: &str, k: &str| v[sec][k].as_array().cloned().unwrap_or_default();
    let (ht, hc, hv, hd) = (
        arr("hourly", "time"),
        arr("hourly", "weather_code"),
        arr("hourly", "temperature_2m"),
        arr("hourly", "is_day"),
    );
    let hourly = ht
        .iter()
        .zip(&hc)
        .zip(&hv)
        .enumerate()
        .filter_map(|(i, ((t, c), x))| {
            Some(Hour {
                hour: t.as_str()?.get(11..13)?.parse().ok()?,
                code: c.as_u64()? as u32,
                temp: f(x)?,
                day: hd.get(i).and_then(|d| d.as_u64()) != Some(0),
            })
        })
        .collect();
    let (dt, dc) = (arr("daily", "time"), arr("daily", "weather_code"));
    let (dmin, dmax) = (
        arr("daily", "temperature_2m_min"),
        arr("daily", "temperature_2m_max"),
    );
    let daily = dt
        .iter()
        .zip(&dc)
        .zip(dmin.iter().zip(&dmax))
        .filter_map(|((t, c), (lo, hi))| {
            Some(Day {
                weekday: weekday(parse_date(t.as_str()?)?),
                code: c.as_u64()? as u32,
                min: f(lo)?,
                max: f(hi)?,
            })
        })
        .collect();
    Some(Weather {
        city: city.to_string(),
        temp: f(&c["temperature_2m"])?,
        code: c["weather_code"].as_u64()? as u32,
        day: c["is_day"].as_u64() != Some(0),
        hourly,
        daily,
    })
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::*;
    use std::sync::Mutex;

    /// The newest query asked for, and the newest answer: (query, places).
    static SUGGEST: Mutex<(String, (String, Vec<Place>))> =
        Mutex::new((String::new(), (String::new(), Vec::new())));

    /// Looks up places for a half-typed city on another thread. `done` runs once they're in,
    /// unless a newer lookup was asked for meanwhile.
    pub fn suggest(query: String, done: impl FnOnce() + Send + 'static) {
        SUGGEST.lock().unwrap().0 = query.clone();
        std::thread::spawn(move || {
            let places = crate::http::curl(&geocode_url(&query, 5), None)
                .map(|b| parse_places(&b))
                .unwrap_or_default();
            let mut s = SUGGEST.lock().unwrap();
            if s.0 != query {
                return;
            }
            s.1 = (query, places);
            drop(s);
            done();
        });
    }

    /// The places found for `query`. Empty until its lookup is back.
    pub fn suggestions(query: &str) -> Vec<Place> {
        let s = SUGGEST.lock().unwrap();
        if s.1 .0 == query {
            s.1 .1.clone()
        } else {
            vec![]
        }
    }
}

/// WMO weather code to a label and an emoji.
pub fn wmo(code: u32, day: bool) -> (&'static str, &'static str) {
    match code {
        0 => ("Clear", if day { "☀️" } else { "🌙" }),
        1 => ("Mostly clear", if day { "🌤️" } else { "🌙" }),
        2 => ("Partly cloudy", if day { "⛅" } else { "☁️" }),
        3 => ("Overcast", "☁️"),
        45 | 48 => ("Fog", "🌫️"),
        51..=57 => ("Drizzle", "🌦️"),
        61..=67 => ("Rain", "🌧️"),
        71..=77 => ("Snow", "🌨️"),
        80..=82 => ("Showers", "🌦️"),
        85 | 86 => ("Snow showers", "🌨️"),
        95..=99 => ("Thunderstorm", "⛈️"),
        _ => ("—", "☁️"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_forecast() {
        let b = r#"{"current":{"temperature_2m":19.2,"weather_code":2,"is_day":1},
            "hourly":{"time":["2026-11-15T17:00","2026-11-15T18:00"],"temperature_2m":[19.0,17.5],"weather_code":[2,3],"is_day":[1,0]},
            "daily":{"time":["2026-11-15","2026-11-16"],"weather_code":[2,61],"temperature_2m_max":[21.0,18.0],"temperature_2m_min":[12.0,11.0]}}"#;
        let w = parse_weather(b, "San Francisco").unwrap();
        assert_eq!((w.temp, w.code, w.day), (19.2, 2, true));
        assert_eq!(
            w.hourly,
            vec![
                Hour {
                    hour: 17,
                    code: 2,
                    temp: 19.0,
                    day: true
                },
                Hour {
                    hour: 18,
                    code: 3,
                    temp: 17.5,
                    day: false
                }
            ]
        );
        assert_eq!(
            w.daily,
            vec![
                Day {
                    weekday: 0,
                    code: 2,
                    min: 12.0,
                    max: 21.0
                },
                Day {
                    weekday: 1,
                    code: 61,
                    min: 11.0,
                    max: 18.0
                }
            ]
        );
        assert_eq!(wmo(w.code, w.day).0, "Partly cloudy");
    }

    #[test]
    fn city_suggestions() {
        let b = r#"{"results":[
            {"name":"Bangkok","latitude":13.75,"longitude":100.5,"admin1":"Bangkok","country":"Thailand"},
            {"name":"Paris","latitude":33.66,"longitude":-95.56,"admin1":"Texas","country":"United States"},
            {"name":"Nowhere"}]}"#;
        let p = parse_places(b);
        assert_eq!(p.len(), 2); // no coordinates, no place
        assert_eq!(
            (p[0].name.as_str(), p[0].area.as_str(), p[0].lat),
            ("Bangkok", "Thailand", 13.75)
        );
        assert_eq!(p[1].area, "Texas, United States");
        assert!(parse_places(r#"{"generationtime_ms":0.5}"#).is_empty()); // what the API sends for no match
    }
}
