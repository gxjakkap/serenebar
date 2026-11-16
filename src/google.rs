//! Google Calendar: sign-in the way Google wants desktop apps to do it (the browser, a loopback
//! redirect and a PKCE challenge), then read-only access to the calendars you have ticked.
//! The refresh token is encrypted with DPAPI, so only this Windows account can read it.
use crate::calendar::{Event, Stamp};
use crate::dates::{days_from_civil, iso_to_unix};
use crate::http::{pct, pct_decode};
use serde_json::Value;

pub const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";

/// Google's consent page for a desktop app with a loopback redirect and a PKCE challenge.
pub fn google_auth_url(client_id: &str, redirect: &str, challenge: &str, state: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}\
         &code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent",
        pct(client_id), pct(redirect), pct(GOOGLE_SCOPE)
    )
}

/// A query parameter from an HTTP request line like `GET /?code=4%2F0A&state=x HTTP/1.1`.
pub fn query_param(request_line: &str, key: &str) -> Option<String> {
    let target = request_line.split_whitespace().nth(1)?;
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| pct_decode(v))
    })
}

/// (access token, refresh token if Google sent one, seconds until the access token expires)
pub fn parse_token(body: &str) -> Option<(String, Option<String>, i64)> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some((
        v["access_token"].as_str()?.to_string(),
        v["refresh_token"].as_str().map(String::from),
        v["expires_in"].as_i64().unwrap_or(3600),
    ))
}

/// Calendars ticked in Google Calendar's sidebar.
pub fn parse_calendar_ids(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return vec![];
    };
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| {
            c["selected"].as_bool().unwrap_or(false) || c["primary"].as_bool().unwrap_or(false)
        })
        .filter_map(|c| c["id"].as_str().map(String::from))
        .collect()
}

/// The signed-in account's address: the primary calendar's ID is the account's email.
pub fn primary_email(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v["items"]
        .as_array()?
        .iter()
        .find(|c| c["primary"].as_bool() == Some(true))?["id"]
        .as_str()
        .map(String::from)
}

/// What to tell the user when the Calendar API answers with an error instead of data.
pub fn api_error(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let e = v.get("error")?;
    let text = e.to_string();
    Some(
        if text.contains("SERVICE_DISABLED") || text.contains("accessNotConfigured") {
            "Google Calendar API is off. Enable it in your Cloud project, then wait a minute."
                .into()
        } else if text.contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT")
            || text.contains("insufficientPermissions")
        {
            "Calendar access wasn't allowed. Sign out, sign in and tick the calendar box.".into()
        } else {
            format!(
                "Google Calendar: {}",
                e["message"].as_str().unwrap_or("request failed")
            )
        },
    )
}

/// Events from Google's events.list. Timed events come back as UTC stamps, all-day ones as dates.
pub fn parse_google_events(body: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return vec![];
    };
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["status"].as_str() != Some("cancelled"))
        .filter_map(|e| {
            let start = &e["start"];
            let at = if let Some(t) = start["dateTime"].as_str().and_then(iso_to_unix) {
                Stamp {
                    day: t.div_euclid(86400),
                    sec: Some(t.rem_euclid(86400)),
                    utc: true,
                }
            } else {
                let d = start["date"].as_str()?;
                Stamp {
                    day: days_from_civil(
                        d.get(0..4)?.parse().ok()?,
                        d.get(5..7)?.parse().ok()?,
                        d.get(8..10)?.parse().ok()?,
                    ),
                    sec: None,
                    utc: false,
                }
            };
            let text = |k: &str| e[k].as_str().unwrap_or("").to_string();
            Some(Event {
                at,
                summary: if text("summary").is_empty() {
                    "(No title)".into()
                } else {
                    text("summary")
                },
                location: text("location"),
            })
        })
        .collect()
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::*;
    use crate::http::{base64url, curl_any, curl_form};
    use crate::settings::Settings;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::Cryptography::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    static STATUS: Mutex<String> = Mutex::new(String::new());
    static ACCOUNT: Mutex<String> = Mutex::new(String::new()); // known after the first fetch
    static ACCESS: Mutex<Option<(String, i64)>> = Mutex::new(None); // access token and when it expires

    const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

    fn set_status(s: &str) {
        *STATUS.lock().unwrap() = s.to_string();
    }

    /// What the settings window shows under the sign-in button. Empty when there's nothing to say.
    pub fn status() -> String {
        STATUS.lock().unwrap().clone()
    }

    fn token_path() -> std::path::PathBuf {
        Settings::path().with_file_name("google.token")
    }

    /// The signed-in account's email, empty until the first calendar fetch.
    pub fn account() -> String {
        ACCOUNT.lock().unwrap().clone()
    }

    pub fn connected() -> bool {
        token_path().exists()
    }

    fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            if protect {
                CryptProtectData(
                    &input,
                    None,
                    None,
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
                .ok()?;
            } else {
                CryptUnprotectData(
                    &input,
                    None,
                    None,
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
                .ok()?;
            }
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(HLOCAL(out.pbData as _));
            Some(bytes)
        }
    }

    fn save_refresh(token: &str) -> bool {
        dpapi(token.as_bytes(), true).is_some_and(|b| std::fs::write(token_path(), b).is_ok())
    }

    fn load_refresh() -> Option<String> {
        String::from_utf8(dpapi(&std::fs::read(token_path()).ok()?, false)?).ok()
    }

    fn random(n: usize) -> String {
        let mut b = vec![0u8; n];
        unsafe {
            let _ = BCryptGenRandom(None, &mut b, BCRYPT_USE_SYSTEM_PREFERRED_RNG);
        }
        base64url(&b)
    }

    fn sha256(data: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        unsafe {
            let _ = BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out);
        }
        out
    }

    /// Opens Google's consent page and waits up to five minutes for the browser to come back.
    /// `done` runs on the sign-in thread once it finishes, either way.
    pub fn sign_in(id: String, secret: String, done: impl FnOnce() + Send + 'static) {
        set_status("Waiting for Google in your browser…");
        std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }; // ShellExecute wants COM
            let ok = run_sign_in(&id, &secret);
            set_status(if ok {
                ""
            } else {
                "Sign-in didn't finish. Check the client ID and secret, then try again."
            });
            done();
        });
    }

    fn run_sign_in(id: &str, secret: &str) -> bool {
        let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
            return false;
        };
        let Ok(port) = listener.local_addr().map(|a| a.port()) else {
            return false;
        };
        let redirect = format!("http://127.0.0.1:{port}");
        let verifier = random(48);
        let challenge = base64url(&sha256(verifier.as_bytes()));
        let state = random(16);
        crate::system::open(&google_auth_url(id, &redirect, &challenge, &state));

        // browsers also ask for /favicon.ico and the like, so wait for the request that carries the result
        let _ = listener.set_nonblocking(true);
        let deadline = Instant::now() + Duration::from_secs(300);
        let code = loop {
            if Instant::now() > deadline {
                return false;
            }
            let Ok((mut conn, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            };
            let _ = conn.set_nonblocking(false);
            let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            let _ = BufReader::new(&conn).read_line(&mut line);
            let code = query_param(&line, "code");
            let error = query_param(&line, "error");
            if code.is_none() && error.is_none() {
                let _ = conn.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                continue;
            }
            let good =
                code.is_some() && query_param(&line, "state").as_deref() == Some(state.as_str());
            let msg = if good {
                "Signed in. You can close this tab and go back to serenebar."
            } else {
                "Sign-in was cancelled. You can close this tab."
            };
            let body = format!("<!doctype html><meta charset=utf-8><title>serenebar</title><body style=\"font:16px system-ui;margin:3em\">{msg}</body>");
            let _ = write!(conn, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            if !good {
                return false;
            }
            break code.unwrap();
        };

        let form = format!(
            "code={}&client_id={}&client_secret={}&redirect_uri={}&grant_type=authorization_code&code_verifier={verifier}",
            pct(&code), pct(id), pct(secret), pct(&redirect)
        );
        let Some((access, Some(refresh), expires)) =
            curl_form(TOKEN_URL, &form).and_then(|b| parse_token(&b))
        else {
            return false;
        };
        *ACCESS.lock().unwrap() = Some((access, crate::dates::unix_now() + expires - 60));
        save_refresh(&refresh)
    }

    /// Forgets the account here and asks Google to revoke the token.
    /// `done` runs on the sign-out thread once it finishes.
    pub fn sign_out(done: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            if let Some(t) = load_refresh() {
                let _ = curl_form(
                    "https://oauth2.googleapis.com/revoke",
                    &format!("token={}", pct(&t)),
                );
            }
            let _ = std::fs::remove_file(token_path());
            *ACCESS.lock().unwrap() = None;
            ACCOUNT.lock().unwrap().clear();
            set_status("");
            done();
        });
    }

    fn access_token(id: &str, secret: &str) -> Option<String> {
        if let Some((t, exp)) = ACCESS.lock().unwrap().clone() {
            if exp > crate::dates::unix_now() {
                return Some(t);
            }
        }
        let form = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}",
            pct(&load_refresh()?),
            pct(id),
            pct(secret)
        );
        let Some((t, _, expires)) = curl_form(TOKEN_URL, &form).and_then(|b| parse_token(&b))
        else {
            set_status("Google sign-in expired. Sign in again.");
            return None;
        };
        *ACCESS.lock().unwrap() = Some((t.clone(), crate::dates::unix_now() + expires - 60));
        Some(t)
    }

    /// Events between days `from` and `to` from every calendar ticked in Google Calendar.
    /// None when not signed in or Google can't be reached.
    pub fn events(id: &str, secret: &str, from: i64, to: i64) -> Option<Vec<Event>> {
        if id.is_empty() || !connected() {
            return None;
        }
        let token = access_token(id, secret)?;
        let auth = format!("Authorization: Bearer {token}\r\n");
        let api = "https://www.googleapis.com/calendar/v3";
        // curl_any so Google's reason for an error reaches Settings instead of vanishing
        let get = |url: &str| {
            let Some(body) = curl_any(url, Some(&auth)) else {
                set_status("Can't reach Google Calendar.");
                return None;
            };
            if let Some(msg) = api_error(&body) {
                set_status(&msg);
                return None;
            }
            Some(body)
        };
        let list = get(&format!("{api}/users/me/calendarList?minAccessRole=reader"))?;
        if let Some(email) = primary_email(&list) {
            *ACCOUNT.lock().unwrap() = email;
        }
        let mut ids = parse_calendar_ids(&list);
        if ids.is_empty() {
            ids.push("primary".into());
        }
        let (min, max) = (
            pct(&crate::dates::unix_to_iso(from * 86400)),
            pct(&crate::dates::unix_to_iso(to * 86400)),
        );
        let mut out = vec![];
        for cal in ids {
            let url = format!("{api}/calendars/{}/events?timeMin={min}&timeMax={max}&singleEvents=true&orderBy=startTime&maxResults=250", pct(&cal));
            if let Some(body) = get(&url) {
                out.extend(parse_google_events(&body));
            }
        }
        set_status(""); // the account works, so any old error is stale
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_pieces() {
        let line = "GET /?state=abc&code=4%2F0AQ&scope=x HTTP/1.1";
        assert_eq!(query_param(line, "code").as_deref(), Some("4/0AQ"));
        assert_eq!(query_param(line, "state").as_deref(), Some("abc"));
        assert_eq!(query_param("GET /favicon.ico HTTP/1.1", "code"), None);
        let url = google_auth_url(
            "id.apps.googleusercontent.com",
            "http://127.0.0.1:5000",
            "CH",
            "ST",
        );
        assert!(
            url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5000")
                && url
                    .contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcalendar.readonly")
        );
        assert_eq!(
            parse_token(r#"{"access_token":"a","expires_in":3599,"refresh_token":"r"}"#),
            Some(("a".into(), Some("r".into()), 3599))
        );
        assert_eq!(parse_token(r#"{"error":"invalid_grant"}"#), None);
    }

    #[test]
    fn google_events_and_calendars() {
        let cal = r#"{"items":[{"id":"me@gmail.com","primary":true},{"id":"holidays#x","selected":true},{"id":"hidden","selected":false}]}"#;
        assert_eq!(parse_calendar_ids(cal), ["me@gmail.com", "holidays#x"]);
        assert_eq!(primary_email(cal).as_deref(), Some("me@gmail.com"));
        assert_eq!(primary_email(r#"{"items":[{"id":"x"}]}"#), None);
        let ev = r#"{"items":[
            {"summary":"Standup","location":"Zoom","start":{"dateTime":"2026-11-16T10:00:00+07:00"}},
            {"summary":"Gone","status":"cancelled","start":{"dateTime":"2026-11-16T11:00:00Z"}},
            {"start":{"date":"2026-11-20"}}]}"#;
        let e = parse_google_events(ev);
        assert_eq!(e.len(), 2);
        assert_eq!(
            e[0].at,
            Stamp {
                day: days_from_civil(2026, 11, 16),
                sec: Some(3 * 3600),
                utc: true
            }
        ); // 10:00 +07 is 03:00 UTC
        assert_eq!(
            (e[0].summary.as_str(), e[0].location.as_str()),
            ("Standup", "Zoom")
        );
        assert_eq!((e[1].at.sec, e[1].summary.as_str()), (None, "(No title)"));
    }

    #[test]
    fn api_errors() {
        let off = r#"{"error":{"code":403,"message":"Google Calendar API has not been used in project 1","errors":[{"reason":"accessNotConfigured"}],"status":"PERMISSION_DENIED"}}"#;
        assert!(api_error(off).unwrap().contains("API is off"));
        let scope = r#"{"error":{"code":403,"message":"Request had insufficient authentication scopes.","details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#;
        assert!(api_error(scope).unwrap().contains("tick the calendar box"));
        assert_eq!(
            api_error(r#"{"error":{"message":"x"}}"#).as_deref(),
            Some("Google Calendar: x")
        );
        assert_eq!(api_error(r#"{"items":[]}"#), None);
    }
}
