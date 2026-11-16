//! Claude and Codex plan limits.
use crate::dates::iso_to_unix;
use serde_json::Value;

/// Which allowance a limit counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Window {
    Session,
    Weekly,
    Opus,
    Sonnet,
}

impl Window {
    pub fn label(self) -> &'static str {
        ["Session", "Weekly", "Opus", "Sonnet"][self as usize]
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
            "refresh_token": oauth["refreshToken"].as_str()?,
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

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use crate::dates::unix_now;
    use crate::http::{curl, curl_json};
    use crate::limits::{apply_claude_refresh, claude_refresh_body, Limit, CLAUDE_TOKEN_URL};

    fn home() -> std::path::PathBuf {
        std::env::var("USERPROFILE").unwrap_or_default().into()
    }

    /// Uses the sign-in Claude Code already keeps on this PC, refreshing it when it has expired
    /// (it only lasts hours) and saving it back for Claude Code. The tokens only go to Anthropic.
    pub fn claude_limits() -> Option<Vec<Limit>> {
        let dir = std::env::var("CLAUDE_CONFIG_DIR")
            .map(Into::into)
            .unwrap_or_else(|_| home().join(".claude"));
        let path = dir.join(".credentials.json");
        let mut creds: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
        let oauth = &creds["claudeAiOauth"];
        // a minute early, so it can't run out between here and the request
        let token = if oauth["expiresAt"]
            .as_i64()
            .is_some_and(|ms| ms / 1000 < unix_now() + 60)
        {
            let body = curl_json(CLAUDE_TOKEN_URL, &claude_refresh_body(oauth)?)?;
            let token = apply_claude_refresh(&mut creds, &body, unix_now() * 1000)?;
            // write then rename, so Claude Code never reads half a file
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_string(&creds).ok()?).ok()?;
            std::fs::rename(&tmp, &path).ok()?;
            token
        } else {
            oauth["accessToken"].as_str()?.to_string()
        };
        let headers =
            format!("Authorization: Bearer {token}\r\nanthropic-beta: oauth-2025-04-20\r\n");
        crate::limits::parse_claude(&curl(
            "https://api.anthropic.com/api/oauth/usage",
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
}
