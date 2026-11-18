//! Updates: the latest GitHub release against this build's version, and installing its setup
//! exe silently in place.
use serde_json::Value;

pub const LATEST_URL: &str = "https://api.github.com/repos/gxjakkap/serenebar/releases/latest";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String, // without the leading v
    pub url: String,     // the release page
    pub setup: Option<Setup>,
}

/// The release's installer, from its assets.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub url: String,
    pub sha256: String, // lowercase hex, from GitHub's digest of the upload
}

/// Body of GitHub's releases/latest, which skips drafts and pre-releases.
pub fn parse_latest(body: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(body).ok()?;
    // releases from before GitHub kept digests have none; those update from the release page
    let setup = v["assets"].as_array().and_then(|a| {
        a.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            (name.starts_with("serenebar-setup-") && name.ends_with("-x64.exe")).then_some(())?;
            Some(Setup {
                url: a["browser_download_url"].as_str()?.to_string(),
                sha256: a["digest"]
                    .as_str()?
                    .strip_prefix("sha256:")?
                    .to_lowercase(),
            })
        })
    });
    Some(Release {
        version: v["tag_name"].as_str()?.trim_start_matches('v').to_string(),
        url: v["html_url"].as_str()?.to_string(),
        setup,
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

/// Whether `r` is newer than this build.
pub fn is_update(r: &Release) -> bool {
    is_newer(&r.version, env!("CARGO_PKG_VERSION"))
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::{parse_latest, Release, LATEST_URL};
    use std::path::{Path, PathBuf};

    /// The latest release, None when GitHub couldn't be reached.
    pub fn latest() -> Option<Release> {
        parse_latest(&crate::http::curl(LATEST_URL, None)?)
    }

    /// Downloads the release's installer to the temp folder. None unless it matches GitHub's
    /// digest.
    pub fn download(r: &Release) -> Option<PathBuf> {
        let setup = r.setup.as_ref()?;
        let path = std::env::temp_dir().join("serenebar-update.exe");
        let ok = crate::http::download(&setup.url, &path)
            && std::fs::read(&path).is_ok_and(|b| sha256_hex(&b) == setup.sha256);
        if !ok {
            let _ = std::fs::remove_file(&path);
        }
        ok.then_some(path)
    }

    fn sha256_hex(b: &[u8]) -> String {
        use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
        let mut h = [0u8; 32];
        if unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, b, &mut h) }.is_err() {
            return String::new();
        }
        h.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// Runs the installer silently. It closes this app (which restores the Windows taskbar),
    /// replaces the exe and starts the new one. `startup` keeps "Launch at startup" as it is,
    /// since setup would otherwise reapply the choice made at first install.
    pub fn install(path: &Path, startup: bool) -> bool {
        let tasks = if startup {
            "/MERGETASKS=startup"
        } else {
            "/MERGETASKS=!startup"
        };
        std::process::Command::new(path)
            .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", tasks])
            .spawn()
            .is_ok()
    }

    /// No input for ten minutes and nothing full-screen (a game, a video, a presentation), so
    /// the bar restarting goes unnoticed.
    pub fn away() -> bool {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_ACCEPTS_NOTIFICATIONS};
        let mut li = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        unsafe {
            GetLastInputInfo(&mut li).as_bool()
                && GetTickCount().wrapping_sub(li.dwTime) >= 10 * 60 * 1000
                && SHQueryUserNotificationState() == Ok(QUNS_ACCEPTS_NOTIFICATIONS)
        }
    }
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
                setup: None,
            })
        );
        let b = r#"{"tag_name":"v0.2.0","html_url":"x","assets":[
            {"name":"notes.txt","browser_download_url":"n","digest":"sha256:00"},
            {"name":"serenebar-setup-0.2.0-x64.exe","browser_download_url":"https://github.com/s.exe","digest":"sha256:ABC123"}]}"#;
        assert_eq!(
            parse_latest(b).unwrap().setup,
            Some(Setup {
                url: "https://github.com/s.exe".into(),
                sha256: "abc123".into(),
            })
        );
        // an asset without a digest can't be checked, so it isn't offered
        let b = r#"{"tag_name":"v0.2.0","html_url":"x","assets":[
            {"name":"serenebar-setup-0.2.0-x64.exe","browser_download_url":"u","digest":null}]}"#;
        assert_eq!(parse_latest(b).unwrap().setup, None);
        assert_eq!(parse_latest(r#"{"message":"Not Found"}"#), None);
    }
}
