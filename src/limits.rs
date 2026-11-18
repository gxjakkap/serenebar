//! AI plan limits: Claude, Codex, Antigravity and Cursor, and the DeepSeek balance.
use crate::dates::iso_to_unix;
use serde_json::Value;

/// Which allowance a limit counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Window {
    Session,
    Weekly,
    Opus,
    Sonnet,
    /// Antigravity's pool for the models that aren't Gemini.
    OtherSession,
    OtherWeekly,
    /// Cursor: the plan's monthly allowance, and its two halves.
    Monthly,
    Auto,
    Api,
}

impl Window {
    pub fn label(self) -> &'static str {
        [
            "Session",
            "Weekly",
            "Opus",
            "Sonnet",
            "Claude and GPT, session",
            "Claude and GPT, weekly",
            "Monthly",
            "Auto",
            "API",
        ][self as usize]
    }

    /// The label on the bar, where there's only room for a word.
    pub fn short(self) -> &'static str {
        match self {
            Window::OtherSession | Window::OtherWeekly => "Others",
            w => w.label(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    pub window: Window,
    pub pct: f32,            // 0 to 100
    pub resets: Option<i64>, // unix seconds
}

/// Body of Claude's `GET https://api.anthropic.com/api/oauth/usage`.
pub fn parse_claude(body: &str) -> Option<Vec<Limit>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let out: Vec<Limit> = [
        ("five_hour", Window::Session),
        ("seven_day", Window::Weekly),
        ("seven_day_opus", Window::Opus),
        ("seven_day_sonnet", Window::Sonnet),
    ]
    .into_iter()
    .filter_map(|(k, window)| {
        let w = &v[k];
        Some(Limit {
            window,
            pct: w["utilization"].as_f64()? as f32,
            resets: w["resets_at"].as_str().and_then(iso_to_unix),
        })
    })
    .collect();
    (!out.is_empty()).then_some(out)
}

/// Claude Code's own OAuth client, which its sign-in in `.credentials.json` belongs to.
const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const CLAUDE_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";

/// The refresh request Claude Code itself sends, from the `claudeAiOauth` part of its credentials.
pub fn claude_refresh_body(oauth: &Value) -> Option<String> {
    let scope: Vec<&str> = oauth["scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    Some(
        serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": oauth["refreshToken"].as_str().filter(|t| !t.is_empty())?,
            "client_id": CLAUDE_CLIENT_ID,
            "scope": scope.join(" "),
        })
        .to_string(),
    )
}

/// Puts a refresh response into the credentials the way Claude Code does, keeping every other
/// field. Refresh tokens rotate, so the new one has to be saved or Claude Code gets signed out.
pub fn apply_claude_refresh(creds: &mut Value, body: &str, now_ms: i64) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let access = v["access_token"].as_str()?.to_string();
    let oauth = creds.get_mut("claudeAiOauth")?.as_object_mut()?;
    oauth.insert("accessToken".into(), access.clone().into());
    if let Some(r) = v["refresh_token"].as_str() {
        oauth.insert("refreshToken".into(), r.into());
    }
    let expires = now_ms + v["expires_in"].as_i64().unwrap_or(3600) * 1000;
    oauth.insert("expiresAt".into(), expires.into());
    Some(access)
}

/// Claude Code keeps its sign-in in Windows Credential Manager (through Bun.secrets), and a value
/// longer than this many bytes is split: base64 in chunks `#0`, `#1`… and `#m` saying `{n, l}`.
pub const CRED_CHUNK: usize = 2400;

/// `(chunks, base64 length)` from the `#m` entry.
pub fn cred_chunk_meta(meta: &[u8]) -> Option<(usize, usize)> {
    let v: Value = serde_json::from_slice(meta).ok()?;
    Some((v["n"].as_u64()? as usize, v["l"].as_u64()? as usize))
}

pub fn cred_join(chunks: &[Vec<u8>], len: usize) -> Option<String> {
    let b64 = String::from_utf8(chunks.concat()).ok()?;
    if b64.len() != len {
        return None;
    }
    String::from_utf8(crate::http::base64_decode(&b64)?).ok()
}

/// The `#m` entry and the chunks for a value too long for one entry, None when it fits in one.
pub fn cred_split(value: &str) -> Option<(String, Vec<String>)> {
    if value.len() <= CRED_CHUNK {
        return None;
    }
    let b64 = crate::http::base64(value.as_bytes());
    let chunks: Vec<String> = b64
        .as_bytes()
        .chunks(CRED_CHUNK)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    let meta = serde_json::json!({"n": chunks.len(), "l": b64.len()}).to_string();
    Some((meta, chunks))
}

fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|x| find(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find(x, key)),
        _ => None,
    }
}

/// One line of a Codex CLI session log. Field names vary across Codex versions, so both spellings are accepted.
pub fn parse_codex_line(line: &str) -> Option<Vec<Limit>> {
    if !line.contains("ate_limits") && !line.contains("ateLimits") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let rl = find(&v, "rate_limits").or_else(|| find(&v, "rateLimits"))?;
    let at = v["timestamp"].as_str().and_then(iso_to_unix);
    let out: Vec<Limit> = ["primary", "secondary"]
        .into_iter()
        .filter_map(|k| {
            let w = &rl[k];
            let pct = w["used_percent"].as_f64().or(w["usedPercent"].as_f64())?;
            let mins = w["window_minutes"]
                .as_i64()
                .or(w["windowDurationMins"].as_i64())
                .unwrap_or(if k == "primary" { 300 } else { 10080 });
            let resets = w["resets_at"]
                .as_i64()
                .or(w["resetsAt"].as_i64())
                .or_else(|| Some(at? + w["resets_in_seconds"].as_i64()?));
            Some(Limit {
                window: if mins <= 24 * 60 {
                    Window::Session
                } else {
                    Window::Weekly
                },
                pct: pct as f32,
                resets,
            })
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// Antigravity's `retrieveUserQuotaSummary`: groups of buckets like `gemini-5h` and
/// `3p-weekly`, each with the fraction left. Gemini first, then each session before its week.
pub fn parse_antigravity(body: &str) -> Option<Vec<Limit>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let mut out: Vec<Limit> = v["groups"]
        .as_array()?
        .iter()
        .flat_map(|g| g["buckets"].as_array().into_iter().flatten())
        .filter(|b| b["disabled"].as_bool() != Some(true))
        .filter_map(|b| {
            let id = b["bucketId"].as_str().unwrap_or("");
            let five = id.ends_with("5h") || b["window"].as_str() == Some("5h");
            let window = match (id.starts_with("gemini"), five) {
                (true, true) => Window::Session,
                (true, false) => Window::Weekly,
                (false, true) => Window::OtherSession,
                (false, false) => Window::OtherWeekly,
            };
            let left = b["remainingFraction"].as_f64()?;
            Some(Limit {
                window,
                pct: ((1.0 - left) * 100.0).clamp(0.0, 100.0) as f32,
                resets: b["resetTime"].as_str().and_then(iso_to_unix),
            })
        })
        .collect();
    out.sort_by_key(|l| l.window as usize);
    out.dedup_by_key(|l| l.window);
    (!out.is_empty()).then_some(out)
}

/// (access token, refresh token, unix expiry) from the agy CLI's stored sign-in:
/// `{"token": {"access_token", "refresh_token", "expiry"}}`.
pub fn parse_agy_token(blob: &str) -> Option<(String, String, i64)> {
    let v: Value = serde_json::from_str(blob).ok()?;
    let t = if v["token"].is_object() {
        &v["token"]
    } else {
        &v
    };
    Some((
        t["access_token"].as_str().unwrap_or("").to_string(),
        t["refresh_token"].as_str()?.to_string(),
        t["expiry"].as_str().and_then(iso_to_unix).unwrap_or(0),
    ))
}

/// The Google Cloud project `loadCodeAssist` assigns, which the quota call asks about.
pub fn code_assist_project(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let p = &v["cloudaicompanionProject"];
    p.as_str().or(p["id"].as_str()).map(String::from)
}

/// Every pairing of the OAuth client IDs and secrets compiled into a program. agy keeps its
/// own in its executable, and refreshing its sign-in needs the matching pair.
pub fn oauth_clients(bin: &[u8]) -> Vec<(String, String)> {
    let find_all = |needle: &[u8]| -> Vec<usize> {
        bin.windows(needle.len())
            .enumerate()
            .filter(|(_, w)| *w == needle)
            .map(|(i, _)| i)
            .collect()
    };
    let mut ids: Vec<String> = vec![];
    for end in find_all(b".apps.googleusercontent.com") {
        let start = bin[..end]
            .iter()
            .rposition(|c| !(c.is_ascii_digit() || c.is_ascii_lowercase() || *c == b'-'))
            .map_or(0, |i| i + 1);
        let id = String::from_utf8_lossy(&bin[start..end]).into_owned();
        let ok = id.split_once('-').is_some_and(|(num, rest)| {
            num.len() >= 10 && num.bytes().all(|c| c.is_ascii_digit()) && rest.len() >= 16
        });
        let id = format!("{id}.apps.googleusercontent.com");
        if ok && !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut secrets: Vec<String> = vec![];
    for at in find_all(b"GOCSPX-") {
        let tail = &bin[at..(at + 35).min(bin.len())];
        let ok = tail.len() == 35
            && tail[7..]
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-');
        let s = String::from_utf8_lossy(tail).into_owned();
        if ok && !secrets.contains(&s) {
            secrets.push(s);
        }
    }
    ids.iter()
        .flat_map(|i| secrets.iter().map(move |s| (i.clone(), s.clone())))
        .collect()
}

/// Money left on a pay-as-you-go account, in one currency.
#[derive(Clone, Debug, PartialEq)]
pub struct Balance {
    pub currency: String,
    pub total: f64,
}

impl Balance {
    /// "¥110.00", "$5.20", or the code in front for any other currency.
    pub fn text(&self) -> String {
        match self.currency.as_str() {
            "CNY" => format!("¥{:.2}", self.total),
            "USD" => format!("${:.2}", self.total),
            c => format!("{c} {:.2}", self.total),
        }
    }
}

/// DeepSeek's `GET /user/balance`. The amounts come as strings.
pub fn parse_deepseek(body: &str) -> Option<Vec<Balance>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let out: Vec<Balance> = v["balance_infos"]
        .as_array()?
        .iter()
        .filter_map(|b| {
            let total = &b["total_balance"];
            Some(Balance {
                currency: b["currency"].as_str()?.to_string(),
                total: total
                    .as_str()
                    .and_then(|t| t.parse().ok())
                    .or(total.as_f64())?,
            })
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// The claims in a JWT's middle part.
pub fn jwt_claims(jwt: &str) -> Option<Value> {
    let part = jwt.split('.').nth(1)?;
    let b64: String = part
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    serde_json::from_slice(&crate::http::base64_decode(&b64)?).ok()
}

/// Cursor's session token from the bytes of its state database (and its write-ahead log): the
/// value stored after the `cursorAuth/accessToken` key. Old copies linger in freed pages, so
/// the one that expires last wins. Returns (user id, token), the two halves of the cookie.
/// ponytail: a byte scan instead of a SQLite reader; a token too big for one page is missed.
pub fn cursor_token(files: &[&[u8]]) -> Option<(String, String)> {
    const KEY: &[u8] = b"cursorAuth/accessToken";
    let jwt_char = |c: &u8| c.is_ascii_alphanumeric() || b"-_.".contains(c);
    let mut best: Option<(i64, String, String)> = None;
    for bin in files {
        for at in (0..bin.len().saturating_sub(KEY.len())).filter(|&i| bin[i..].starts_with(KEY)) {
            let rest = &bin[at + KEY.len()..];
            if !rest.starts_with(b"eyJ") {
                continue; // the key's index entry, not its row
            }
            let n = rest.iter().take_while(|c| jwt_char(c)).count();
            let jwt = String::from_utf8_lossy(&rest[..n]).into_owned();
            let Some(claims) = jwt_claims(&jwt) else {
                continue;
            };
            let exp = claims["exp"].as_i64().unwrap_or(0);
            // "auth0|user_01…": the cookie wants only the part after the bar
            let Some(user) = claims["sub"].as_str().and_then(|s| s.rsplit('|').next()) else {
                continue;
            };
            if best.as_ref().is_none_or(|b| exp > b.0) {
                best = Some((exp, user.to_string(), jwt));
            }
        }
    }
    best.map(|(_, u, t)| (u, t))
}

/// Cursor's `GET /api/usage-summary`: the plan's use this billing month, overall and split
/// into Auto and the named (API) models.
pub fn parse_cursor(body: &str) -> Option<Vec<Limit>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let plan = &v["individualUsage"]["plan"];
    let resets = v["billingCycleEnd"].as_str().and_then(iso_to_unix);
    let total = plan["totalPercentUsed"].as_f64().or_else(|| {
        let (used, limit) = (plan["used"].as_f64()?, plan["limit"].as_f64()?);
        (limit > 0.0).then(|| used / limit * 100.0)
    })?;
    let mut out = vec![Limit {
        window: Window::Monthly,
        pct: total as f32,
        resets,
    }];
    for (k, window) in [
        ("autoPercentUsed", Window::Auto),
        ("apiPercentUsed", Window::Api),
    ] {
        if let Some(p) = plan[k].as_f64() {
            out.push(Limit {
                window,
                pct: p as f32,
                resets,
            });
        }
    }
    Some(out)
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use crate::dates::unix_now;
    use crate::http::{curl, curl_form, curl_json, curl_post, pct};
    use crate::limits::{
        apply_claude_refresh, claude_refresh_body, code_assist_project, oauth_clients,
        parse_agy_token, parse_antigravity, Limit, CLAUDE_TOKEN_URL,
    };
    use serde_json::Value;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    fn home() -> PathBuf {
        std::env::var("USERPROFILE").unwrap_or_default().into()
    }

    /// Claude Code's entries in Credential Manager, the way Bun.secrets names them.
    mod credman {
        use crate::limits::{cred_chunk_meta, cred_join, cred_split};
        use windows::core::{HSTRING, PWSTR};
        use windows::Win32::Security::Credentials::*;

        const ACCOUNT: &str = "claude-code-user";

        fn target(part: &str) -> HSTRING {
            HSTRING::from(format!("Claude Code-credentials/{ACCOUNT}{part}"))
        }

        fn read(part: &str) -> Option<Vec<u8>> {
            read_generic(&target(part))
        }

        /// A generic credential's secret, as stored.
        pub fn read_generic(target: &HSTRING) -> Option<Vec<u8>> {
            let mut c: *mut CREDENTIALW = std::ptr::null_mut();
            unsafe {
                CredReadW(target, CRED_TYPE_GENERIC, 0, &mut c).ok()?;
                let r = &*c;
                let blob = if r.CredentialBlob.is_null() {
                    vec![]
                } else {
                    std::slice::from_raw_parts(r.CredentialBlob, r.CredentialBlobSize as usize)
                        .to_vec()
                };
                CredFree(c as *const _);
                Some(blob)
            }
        }

        fn write(part: &str, value: &[u8]) -> bool {
            let mut name: Vec<u16> = format!("Claude Code-credentials/{ACCOUNT}{part}")
                .encode_utf16()
                .chain([0])
                .collect();
            let mut user: Vec<u16> = format!("{ACCOUNT}{part}")
                .encode_utf16()
                .chain([0])
                .collect();
            let c = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(name.as_mut_ptr()),
                UserName: PWSTR(user.as_mut_ptr()),
                CredentialBlobSize: value.len() as u32,
                CredentialBlob: value.as_ptr() as *mut u8,
                Persist: CRED_PERSIST_ENTERPRISE, // Bun.secrets' default
                ..Default::default()
            };
            unsafe { CredWriteW(&c, 0).is_ok() }
        }

        fn delete(part: &str) {
            let _ = unsafe { CredDeleteW(&target(part), CRED_TYPE_GENERIC, 0) };
        }

        fn meta() -> Option<(usize, usize)> {
            read("#m").and_then(|m| cred_chunk_meta(&m))
        }

        pub fn load() -> Option<String> {
            match meta() {
                Some((n, len)) => {
                    let chunks: Option<Vec<_>> = (0..n).map(|i| read(&format!("#{i}"))).collect();
                    cred_join(&chunks?, len)
                }
                None => String::from_utf8(read("")?).ok(),
            }
        }

        /// Saves in the order Claude Code does, with `#p` marking a change in progress.
        pub fn save(value: &str) -> bool {
            let old = meta().map_or(0, |(n, _)| n);
            match cred_split(value) {
                None => {
                    if !write("", value.as_bytes()) {
                        return false;
                    }
                    if old > 0 {
                        write("#p", old.to_string().as_bytes());
                        delete("#m");
                        (0..old).for_each(|i| delete(&format!("#{i}")));
                        delete("#p");
                    }
                }
                Some((m, chunks)) => {
                    write("#p", chunks.len().max(old).to_string().as_bytes());
                    let all = chunks
                        .iter()
                        .enumerate()
                        .all(|(i, c)| write(&format!("#{i}"), c.as_bytes()));
                    if !all || !write("#m", m.as_bytes()) {
                        return false;
                    }
                    delete("");
                    (chunks.len()..old).for_each(|i| delete(&format!("#{i}")));
                    delete("#p");
                }
            }
            true
        }
    }

    /// Claude Code's lock for writing its sign-in: a folder, stale after 15 s untouched.
    struct Lock(PathBuf);

    impl Lock {
        fn take(dir: &Path) -> Option<Lock> {
            let p = dir.join(".storage-write.lock");
            if std::fs::create_dir(&p).is_err() {
                let age = std::fs::metadata(&p)
                    .ok()?
                    .modified()
                    .ok()?
                    .elapsed()
                    .ok()?;
                if age.as_secs() < 15 {
                    return None; // Claude Code is writing; try again next round
                }
                std::fs::remove_dir(&p).ok()?;
                std::fs::create_dir(&p).ok()?;
            }
            Some(Lock(p))
        }
    }

    impl Drop for Lock {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    /// Where Claude Code's sign-in is: Credential Manager, else the plaintext file.
    enum Store {
        Credman,
        File(PathBuf),
    }

    impl Store {
        fn load(dir: &Path) -> Option<(Store, Value)> {
            // with either variable set, the entry's name ends in a hash of the folder
            // ponytail: only the default entry is read; the file still works for the others
            let default_dir = std::env::var_os("CLAUDE_CONFIG_DIR").is_none()
                && std::env::var_os("CLAUDE_SECURESTORAGE_CONFIG_DIR").is_none();
            if let Some(v) = default_dir.then(credman::load).flatten() {
                return Some((Store::Credman, serde_json::from_str(&v).ok()?));
            }
            let path = dir.join(".credentials.json");
            let v = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            Some((Store::File(path), v))
        }

        fn save(&self, creds: &Value) -> Option<()> {
            let text = serde_json::to_string(creds).ok()?;
            match self {
                Store::Credman => credman::save(&text).then_some(()),
                Store::File(path) => {
                    // write then rename, so Claude Code never reads half a file
                    let tmp = path.with_extension("json.tmp");
                    std::fs::write(&tmp, text).ok()?;
                    std::fs::rename(&tmp, path).ok()
                }
            }
        }
    }

    /// The access token, or None when it runs out within a minute.
    fn fresh_token(creds: &Value) -> Option<String> {
        let o = &creds["claudeAiOauth"];
        let live = o["expiresAt"]
            .as_i64()
            .is_some_and(|ms| ms / 1000 > unix_now() + 60);
        o["accessToken"]
            .as_str()
            .filter(|t| live && !t.is_empty())
            .map(String::from)
    }

    /// Uses the sign-in Claude Code already keeps on this PC, refreshing it when it has expired
    /// (it only lasts hours) and saving it back for Claude Code. The tokens only go to Anthropic.
    pub fn claude_limits() -> Option<Vec<Limit>> {
        let dir = std::env::var("CLAUDE_CONFIG_DIR")
            .map(Into::into)
            .unwrap_or_else(|_| home().join(".claude"));
        let (_, creds) = Store::load(&dir)?;
        let token = match fresh_token(&creds) {
            Some(t) => t,
            None => {
                let _lock = Lock::take(&dir)?;
                // Claude Code may have refreshed it while we waited
                let (store, mut creds) = Store::load(&dir)?;
                match fresh_token(&creds) {
                    Some(t) => t,
                    None => {
                        let body = claude_refresh_body(&creds["claudeAiOauth"])?;
                        let body = curl_json(CLAUDE_TOKEN_URL, &body)?;
                        let t = apply_claude_refresh(&mut creds, &body, unix_now() * 1000)?;
                        store.save(&creds)?;
                        t
                    }
                }
            }
        };
        let headers =
            format!("Authorization: Bearer {token}\r\nanthropic-beta: oauth-2025-04-20\r\n");
        crate::limits::parse_claude(&curl(
            "https://api.anthropic.com/api/oauth/usage",
            Some(&headers),
        )?)
    }

    /// Antigravity's access token and when it expires, and the OAuth client that refreshed it.
    /// ponytail: a refreshed token stays in memory, not written back for agy; Google keeps a
    /// desktop app's refresh token, so agy's own copy keeps working.
    static AGY: Mutex<Option<(String, i64)>> = Mutex::new(None);
    static AGY_CLIENT: Mutex<Option<(String, String)>> = Mutex::new(None);
    static AGY_PROJECT: Mutex<Option<String>> = Mutex::new(None);

    /// agy.exe where its installer puts it, else on PATH.
    fn agy_exe() -> Option<PathBuf> {
        let local =
            std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"agy\bin\agy.exe"));
        let path = std::env::var_os("PATH").unwrap_or_default();
        local
            .into_iter()
            .chain(std::env::split_paths(&path).map(|d| d.join("agy.exe")))
            .find(|p| p.is_file())
    }

    fn agy_token() -> Option<String> {
        let now = unix_now();
        let cached = AGY.lock().unwrap().clone().filter(|a| a.1 > now + 60);
        if let Some((t, _)) = cached {
            return Some(t);
        }
        let blob = credman::read_generic(&windows::core::HSTRING::from("gemini:antigravity"))?;
        let (access, refresh, exp) = parse_agy_token(&String::from_utf8(blob).ok()?)?;
        if exp > now + 60 && !access.is_empty() {
            *AGY.lock().unwrap() = Some((access.clone(), exp));
            return Some(access);
        }
        let known = AGY_CLIENT.lock().unwrap().clone();
        let clients = match known {
            Some(c) => vec![c],
            None => oauth_clients(&std::fs::read(agy_exe()?).ok()?),
        };
        for (id, secret) in clients {
            let form = format!(
                "grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}",
                pct(&refresh),
                pct(&id),
                pct(&secret)
            );
            let Some((t, _, expires)) = curl_form("https://oauth2.googleapis.com/token", &form)
                .and_then(|b| crate::google::parse_token(&b))
            else {
                continue;
            };
            *AGY_CLIENT.lock().unwrap() = Some((id, secret));
            *AGY.lock().unwrap() = Some((t.clone(), now + expires));
            return Some(t);
        }
        None
    }

    /// Antigravity's plan limits, from the sign-in its CLI (agy) keeps in Credential Manager.
    /// The token only goes to Google.
    pub fn antigravity_limits() -> Option<Vec<Limit>> {
        let token = agy_token()?;
        let headers = format!("Authorization: Bearer {token}\r\nUser-Agent: antigravity\r\n");
        let known = AGY_PROJECT.lock().unwrap().clone();
        let project = match known {
            Some(p) => p,
            None => {
                let meta = r#"{"metadata":{"ideType":"ANTIGRAVITY","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}}"#;
                let url = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
                let p = code_assist_project(&curl_post(url, &headers, meta)?)?;
                *AGY_PROJECT.lock().unwrap() = Some(p.clone());
                p
            }
        };
        let body = serde_json::json!({ "project": project }).to_string();
        // the daily host has the live numbers; the other one can lag at 100% left
        let url = "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary";
        parse_antigravity(&curl_post(url, &headers, &body)?)
    }

    /// Cursor's plan use, with the session Cursor keeps on this PC. The token only goes to Cursor.
    pub fn cursor_limits() -> Option<Vec<Limit>> {
        let db = PathBuf::from(std::env::var_os("APPDATA")?)
            .join(r"Cursor\User\globalStorage\state.vscdb");
        let main = std::fs::read(&db).ok()?;
        let wal = std::fs::read(db.with_extension("vscdb-wal")).unwrap_or_default();
        let (user, token) = crate::limits::cursor_token(&[&main, &wal])?;
        let headers = format!("Cookie: WorkosCursorSessionToken={user}%3A%3A{token}\r\n");
        crate::limits::parse_cursor(&curl(
            "https://cursor.com/api/usage-summary",
            Some(&headers),
        )?)
    }

    /// The balance on a DeepSeek account. The key only goes to DeepSeek.
    pub fn deepseek_balance(key: &str) -> Option<Vec<crate::limits::Balance>> {
        let headers = format!("Authorization: Bearer {key}\r\nAccept: application/json\r\n");
        crate::limits::parse_deepseek(&curl(
            "https://api.deepseek.com/user/balance",
            Some(&headers),
        )?)
    }

    /// Codex CLI writes its rate limits into the session logs, so this reads the newest one.
    /// No network.
    pub fn codex_limits() -> Option<Vec<Limit>> {
        let root = std::env::var("CODEX_HOME")
            .map(Into::into)
            .unwrap_or_else(|_| home().join(".codex"))
            .join("sessions");
        let newest = |p: &std::path::Path| -> Vec<std::path::PathBuf> {
            let mut v: Vec<_> = std::fs::read_dir(p)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            v.sort();
            v.reverse();
            v
        };
        // sessions/YYYY/MM/DD/rollout-*.jsonl, and the names sort by date
        let files = newest(&root)
            .into_iter()
            .flat_map(|y| newest(&y))
            .flat_map(|m| newest(&m))
            .flat_map(|d| newest(&d))
            .filter(|f| f.extension().is_some_and(|x| x == "jsonl"));
        for f in files.take(5) {
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            if let Some(l) = text.lines().rev().find_map(crate::limits::parse_codex_line) {
                return Some(l);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_usage_response() {
        let b = r#"{"five_hour":{"utilization":34.0,"resets_at":"2026-11-15T19:00:00+00:00"},"seven_day":{"utilization":58,"resets_at":null},"seven_day_opus":null}"#;
        let l = parse_claude(b).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].window, l[0].pct), (Window::Session, 34.0));
        assert!(l[0].resets.is_some() && l[1].resets.is_none());
        assert_eq!(parse_claude("{}"), None);
    }

    #[test]
    fn claude_token_refresh() {
        let mut creds = serde_json::json!({"claudeAiOauth": {"accessToken": "old", "refreshToken": "r1",
            "expiresAt": 1, "scopes": ["user:inference", "user:profile"], "subscriptionType": "pro"}, "other": 1});
        let body: Value =
            serde_json::from_str(&claude_refresh_body(&creds["claudeAiOauth"]).unwrap()).unwrap();
        assert_eq!(
            (body["refresh_token"].as_str(), body["scope"].as_str()),
            (Some("r1"), Some("user:inference user:profile"))
        );
        let r = r#"{"access_token":"new","refresh_token":"r2","expires_in":28800}"#;
        assert_eq!(
            apply_claude_refresh(&mut creds, r, 1000).as_deref(),
            Some("new")
        );
        let o = &creds["claudeAiOauth"];
        assert_eq!(
            (o["refreshToken"].as_str(), o["expiresAt"].as_i64()),
            (Some("r2"), Some(28_801_000))
        );
        assert_eq!(
            (o["subscriptionType"].as_str(), creds["other"].as_i64()),
            (Some("pro"), Some(1))
        );
        assert_eq!(apply_claude_refresh(&mut creds, "{}", 0), None);
        // the plaintext file Claude Code leaves when the tokens are in Credential Manager
        assert_eq!(
            claude_refresh_body(&serde_json::json!({"refreshToken": ""})),
            None
        );
    }

    #[test]
    fn credential_manager_chunks() {
        let small = r#"{"claudeAiOauth":{}}"#;
        assert_eq!(cred_split(small), None);
        let big = format!(r#"{{"mcpOAuth":"{}"}}"#, "é".repeat(3000));
        let (meta, chunks) = cred_split(&big).unwrap();
        assert!(chunks.len() == 4 && chunks.iter().all(|c| c.len() <= CRED_CHUNK));
        let (n, len) = cred_chunk_meta(meta.as_bytes()).unwrap();
        let bytes: Vec<Vec<u8>> = chunks.into_iter().map(String::into_bytes).collect();
        assert_eq!(n, bytes.len());
        assert_eq!(cred_join(&bytes, len).as_deref(), Some(big.as_str()));
        assert_eq!(cred_join(&bytes[..3], len), None);
    }

    #[test]
    fn codex_session_log_line() {
        let a = r#"{"timestamp":"2026-11-15T10:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":46.0,"window_minutes":299,"resets_in_seconds":600},"secondary":{"used_percent":62.0,"window_minutes":10080,"resets_at":1763900000}}}}"#;
        let l = parse_codex_line(a).unwrap();
        assert_eq!(
            (l[0].window, l[0].pct, l[0].resets),
            (Window::Session, 46.0, iso_to_unix("2026-11-15T10:10:00Z"))
        );
        assert_eq!(
            (l[1].window, l[1].resets),
            (Window::Weekly, Some(1763900000))
        );
        assert_eq!(parse_codex_line(r#"{"type":"message"}"#), None);
    }

    #[test]
    fn antigravity_quota() {
        let b = r#"{"description":"","groups":[
            {"displayName":"Claude and GPT models","buckets":[
                {"bucketId":"3p-weekly","window":"weekly","remainingFraction":0.9,"resetTime":"2026-11-20T00:00:00Z"},
                {"bucketId":"3p-5h","window":"5h","remainingFraction":1.0,"disabled":true}]},
            {"displayName":"Gemini Models","buckets":[
                {"bucketId":"gemini-weekly","window":"weekly","remainingFraction":0.8127},
                {"bucketId":"gemini-5h","window":"5h","remainingFraction":0.2725,"resetTime":"2026-11-18T15:00:00Z"}]}]}"#;
        let l = parse_antigravity(b).unwrap();
        let w: Vec<Window> = l.iter().map(|l| l.window).collect();
        assert_eq!(w, [Window::Session, Window::Weekly, Window::OtherWeekly]);
        assert!((l[0].pct - 72.75).abs() < 0.01);
        assert_eq!(l[0].resets, iso_to_unix("2026-11-18T15:00:00Z"));
        assert_eq!(parse_antigravity(r#"{"groups":[]}"#), None);

        let blob = r#"{"token":{"access_token":"ya29.a","token_type":"Bearer","refresh_token":"1//r","expiry":"2026-11-18T12:00:00.5+07:00"},"auth_method":"consumer"}"#;
        assert_eq!(
            parse_agy_token(blob),
            Some((
                "ya29.a".into(),
                "1//r".into(),
                iso_to_unix("2026-11-18T05:00:00Z").unwrap()
            ))
        );
        assert_eq!(parse_agy_token(r#"{"token":{}}"#), None);
        assert_eq!(
            code_assist_project(r#"{"cloudaicompanionProject":"p-1"}"#).as_deref(),
            Some("p-1")
        );
        assert_eq!(
            code_assist_project(r#"{"cloudaicompanionProject":{"id":"p-2"}}"#).as_deref(),
            Some("p-2")
        );
    }

    #[test]
    fn oauth_clients_in_a_binary() {
        // made-up values, put together here so they don't read as real credentials
        let host = "googleusercontent";
        let id = format!(
            "{}-{}.apps.{host}.com",
            "1234567890", "abcdefghijklmnop0123"
        );
        let secret = format!("{}-{}", "GOCSPX", "abcdefghijklmnopqrstuvwxyz12");
        let mut bin = format!("\x00junk-12345.apps.{host}.com\x00\x01{id}\x00").into_bytes();
        bin.extend(format!("{secret}\x00{}-short\x00", "GOCSPX").bytes());
        assert_eq!(oauth_clients(&bin), [(id, secret)]);
    }

    #[test]
    fn deepseek_balance_response() {
        let b = r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"110.00","granted_balance":"10.00","topped_up_balance":"100.00"}]}"#;
        let l = parse_deepseek(b).unwrap();
        assert_eq!(l[0].text(), "¥110.00");
        let usd = Balance {
            currency: "USD".into(),
            total: 5.2,
        };
        assert_eq!(usd.text(), "$5.20");
        assert_eq!(parse_deepseek(r#"{"balance_infos":[]}"#), None);
    }

    #[test]
    fn cursor_session_and_usage() {
        let jwt = |sub: &str, exp: i64| {
            let claims =
                crate::http::base64url(format!(r#"{{"sub":"{sub}","exp":{exp}}}"#).as_bytes());
            format!("eyJhbGciOiJIUzI1NiJ9.{claims}.c2ln")
        };
        let (old, new) = (jwt("auth0|user_old", 100), jwt("auth0|user_01ABC", 200));
        let mut db = b"\x05cursorAuth/accessToken\x00\x00".to_vec(); // the index entry
        db.extend(format!("\x07cursorAuth/accessToken{old}\x00").bytes());
        let wal = format!("..cursorAuth/accessToken{new}\x01");
        assert_eq!(
            cursor_token(&[&db, wal.as_bytes()]),
            Some(("user_01ABC".to_string(), new))
        );
        assert_eq!(cursor_token(&[b"nothing here"]), None);

        let b = r#"{"billingCycleStart":"2026-11-02T14:11:55.000Z","billingCycleEnd":"2026-12-02T14:11:55.000Z",
            "individualUsage":{"plan":{"enabled":true,"used":1500,"limit":2000,"autoPercentUsed":20,"apiPercentUsed":55,"totalPercentUsed":75}}}"#;
        let l = parse_cursor(b).unwrap();
        assert_eq!(
            l.iter().map(|l| (l.window, l.pct)).collect::<Vec<_>>(),
            [
                (Window::Monthly, 75.0),
                (Window::Auto, 20.0),
                (Window::Api, 55.0)
            ]
        );
        assert_eq!(l[0].resets, iso_to_unix("2026-12-02T14:11:55Z"));
        let old_shape = r#"{"individualUsage":{"plan":{"used":500,"limit":2000}}}"#;
        assert_eq!(parse_cursor(old_shape).unwrap()[0].pct, 25.0);
        assert_eq!(parse_cursor(r#"{"individualUsage":{}}"#), None);
    }
}
