//! Update check: the latest GitHub release, compared with this build's version.
use serde_json::Value;

pub const LATEST_URL: &str = "https://api.github.com/repos/gxjakkap/serenebar/releases/latest";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String, // without the leading v
    pub url: String,     // the release page
}

/// Body of GitHub's releases/latest, which skips drafts and pre-releases.
pub fn parse_latest(body: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(Release {
        version: v["tag_name"].as_str()?.trim_start_matches('v').to_string(),
        url: v["html_url"].as_str()?.to_string(),
    })
}

/// Whether `latest` is a higher version than `current`, both like 1.2.3 (a -suffix is ignored).
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parts = |s: &str| -> Option<Vec<u64>> {
        let core = s.trim_start_matches('v').split(['-', '+']).next()?;
        core.split('.').map(|p| p.parse().ok()).collect()
    };
    match (parts(latest), parts(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// The latest release, None when GitHub couldn't be reached.
#[cfg(windows)]
pub fn latest() -> Option<Release> {
    parse_latest(&crate::http::curl(LATEST_URL, None)?)
}

/// Whether `r` is newer than this build.
pub fn is_update(r: &Release) -> bool {
    is_newer(&r.version, env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("0.1.0-rc1", "0.1.0"));
    }

    #[test]
    fn latest_release() {
        let b = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/gxjakkap/serenebar/releases/tag/v0.2.0","draft":false}"#;
        assert_eq!(
            parse_latest(b),
            Some(Release {
                version: "0.2.0".into(),
                url: "https://github.com/gxjakkap/serenebar/releases/tag/v0.2.0".into(),
            })
        );
        assert_eq!(parse_latest(r#"{"message":"Not Found"}"#), None);
    }
}
