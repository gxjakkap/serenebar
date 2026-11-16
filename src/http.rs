//! URL encoding, and fetching through the curl.exe that ships with Windows.

/// Percent-encodes everything but letters, digits and `-._~`.
pub fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode(bytes: &[u8], url: bool) -> String {
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for j in 0..=c.len() {
            let ch = B64[(n >> (18 - 6 * j) & 63) as usize];
            out.push(match (url, ch) {
                (true, b'+') => '-',
                (true, b'/') => '_',
                _ => ch as char,
            });
        }
        if !url {
            out.push_str(&"=="[..3 - c.len()]);
        }
    }
    out
}

/// URL-safe base64 without padding, as PKCE wants.
pub fn base64url(bytes: &[u8]) -> String {
    encode(bytes, true)
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    encode(bytes, false)
}

/// Standard base64, padded or not. None for anything else.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let digits: Vec<u32> = s
        .trim_end_matches('=')
        .bytes()
        .map(|b| B64.iter().position(|&a| a == b).map(|i| i as u32))
        .collect::<Option<_>>()?;
    if digits.len() % 4 == 1 {
        return None;
    }
    let mut out = vec![];
    for c in digits.chunks(4) {
        let n = c
            .iter()
            .enumerate()
            .fold(0, |n, (j, d)| n | d << (18 - 6 * j));
        out.extend(n.to_be_bytes()[1..c.len()].iter());
    }
    Some(out)
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    /// `headers` go to curl on stdin (`-H @-`), which keeps tokens out of the process list.
    pub fn curl(url: &str, headers: Option<&str>) -> Option<String> {
        run_curl("-fsS", url, headers.map(|h| (&["-H", "@-"][..], h)))
    }

    /// Like `curl`, but an HTTP error's body comes back too, for APIs that say what went wrong.
    pub fn curl_any(url: &str, headers: Option<&str>) -> Option<String> {
        run_curl("-sS", url, headers.map(|h| (&["-H", "@-"][..], h)))
    }

    /// POSTs a form, sent on stdin for the same reason.
    pub fn curl_form(url: &str, form: &str) -> Option<String> {
        run_curl("-fsS", url, Some((&["--data-binary", "@-"], form)))
    }

    /// POSTs a JSON body, sent on stdin like a form.
    pub fn curl_json(url: &str, json: &str) -> Option<String> {
        let args = [
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
        ];
        run_curl("-fsS", url, Some((&args, json)))
    }

    fn run_curl(fail: &str, url: &str, stdin: Option<(&[&str], &str)>) -> Option<String> {
        use std::io::Write;
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let mut cmd = Command::new("curl.exe");
        cmd.args([fail, "--max-time", "15"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some((args, _)) = stdin {
            cmd.args(args);
        }
        let mut child = cmd.arg(url).spawn().ok()?;
        if let Some((_, text)) = stdin {
            child.stdin.take()?.write_all(text.as_bytes()).ok()?;
        }
        drop(child.stdin.take());
        let out = child.wait_with_output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
        for s in ["", "f", "fo", "foo", "foob", "{\"a\":1}"] {
            assert_eq!(base64_decode(&base64(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert_eq!(base64_decode("Zm9v!"), None);
        assert_eq!(pct("a b/c?d"), "a%20b%2Fc%3Fd");
        assert_eq!(pct("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(pct_decode("4%2F0Ab%20x+y%"), "4/0Ab x y%");
    }
}
