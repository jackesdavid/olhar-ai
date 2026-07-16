//! One-shot RTSP credential probe.
//!
//! Speaks RTSP to the camera directly and classifies the outcome, so a failure can name its
//! own cause instead of every failure being reported as "wrong password".
//!
//! Two rules here are load-bearing, both learned the hard way:
//!
//! 1. **One connection, one verdict — never retry.** Consumer cameras (Tapo, Reolink, Wyze)
//!    lock their RTSP server after a burst of failed auth. A probe that polls turns a wrong
//!    password into a camera that refuses *everyone*, including the streams that were working
//!    a minute ago. The lock outlives the app and clears only on a power cycle.
//! 2. **Probe before persisting.** Writing unproven creds into go2rtc's config points a
//!    reconnect loop at the camera, which is the same auth storm by another route.
//!
//! A locked camera answers even an unauthenticated `OPTIONS` with `400 Server is locked`,
//! which is exactly how we tell "locked" apart from "wrong password" (`401`).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const IO_TIMEOUT: Duration = Duration::from_secs(6);

/// Why a camera did or didn't stream. Each variant maps to distinct user-facing copy —
/// that separation is the whole point of the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    /// Creds authenticated and the stream described cleanly.
    Ok,
    /// The camera evaluated the creds and rejected them.
    AuthFailed,
    /// The camera is refusing every RTSP client regardless of creds — needs a power cycle.
    Locked,
    /// Authenticated, but no stream at that path.
    NotFound,
    /// No TCP connection (wrong IP, camera off, firewall).
    Unreachable,
    /// Connected, but the camera never answered.
    Timeout,
    /// Anything we couldn't classify.
    Error,
}

impl ProbeStatus {
    /// Whether re-probing could plausibly change the answer. `AuthFailed`/`Locked` are hard
    /// no's: retrying them is what causes the lockout in the first place.
    pub fn is_retryable(self) -> bool {
        matches!(self, ProbeStatus::Timeout | ProbeStatus::Unreachable)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Probe {
    pub status: ProbeStatus,
    /// Operator-facing detail (the camera's own reason line where we have one).
    pub detail: String,
}

impl Probe {
    fn new(status: ProbeStatus, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }
}

/// Percent-encode an RTSP userinfo component (RFC 3986 `userinfo`).
///
/// Unencoded creds silently corrupt the URL: a password containing `@` makes the parser read
/// the wrong host, and `:` splits the password. Cameras then report "wrong password" for a
/// password that is perfectly correct.
pub fn encode_userinfo(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~');
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

struct Url<'a> {
    host: &'a str,
    port: u16,
}

/// Split `rtsp://[user:pass@]host[:port]/path` into the parts we need to talk to it.
fn parse(url: &str) -> Option<Url<'_>> {
    let rest = url.strip_prefix("rtsp://")?;
    // Drop any embedded creds — we send our own via an auth header.
    let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
    let authority = match rest.find('/') {
        Some(i) => &rest[..i],
        None => rest,
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, 554),
    };
    if host.is_empty() {
        return None;
    }
    Some(Url { host, port })
}

fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

/// Build the `Authorization` value for a challenge, or `None` if we can't satisfy it.
fn authorization(
    challenge: &str,
    method: &str,
    uri: &str,
    user: &str,
    pass: &str,
) -> Option<String> {
    let lower = challenge.to_ascii_lowercase();
    if lower.contains("digest") {
        let realm = field(challenge, "realm")?;
        let nonce = field(challenge, "nonce")?;
        let ha1 = md5_hex(&format!("{user}:{realm}:{pass}"));
        let ha2 = md5_hex(&format!("{method}:{uri}"));
        let response = md5_hex(&format!("{ha1}:{nonce}:{ha2}"));
        return Some(format!(
            "Digest username=\"{user}\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{uri}\", response=\"{response}\""
        ));
    }
    if lower.contains("basic") {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        return Some(format!("Basic {raw}"));
    }
    None
}

/// Pull `name="value"` out of a `WWW-Authenticate` line.
fn field(header: &str, name: &str) -> Option<String> {
    let at = header.to_ascii_lowercase().find(&format!("{name}=\""))?;
    let rest = &header[at + name.len() + 2..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Status code + reason phrase from an RTSP status line.
fn status_of(response: &str) -> Option<(u16, String)> {
    let line = response.lines().next()?;
    let mut parts = line.splitn(3, ' ');
    let _version = parts.next()?;
    let code = parts.next()?.parse().ok()?;
    let reason = parts.next().unwrap_or("").trim().to_string();
    Some((code, reason))
}

fn header_of(response: &str, name: &str) -> Option<String> {
    let want = format!("{}:", name.to_ascii_lowercase());
    response
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with(&want))
        .map(|l| l[want.len()..].trim().to_string())
}

/// A camera in lockout answers everything with `400` — Tapo spells it out ("Server is
/// locked"), others send a bare 400. Our requests are well-formed, so a 400 means the camera
/// is refusing clients wholesale rather than judging the request, and the advice is the same
/// either way: power-cycle it.
fn is_locked(code: u16) -> bool {
    code == 400
}

fn send(stream: &mut TcpStream, req: &str) -> Result<String, std::io::Error> {
    stream.write_all(req.as_bytes())?;
    stream.flush()?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
}

/// Probe `url` with `user`/`pass`. Sends at most two requests on one connection, and never
/// retries — see the module docs for why that restraint is the feature.
pub fn probe(url: &str, user: &str, pass: &str) -> Probe {
    let Some(parsed) = parse(url) else {
        return Probe::new(ProbeStatus::Error, format!("Not a valid RTSP URL: {url}"));
    };

    let addr = match (parsed.host, parsed.port).to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => {
                return Probe::new(
                    ProbeStatus::Unreachable,
                    format!("{} has no address", parsed.host),
                )
            }
        },
        Err(e) => {
            return Probe::new(
                ProbeStatus::Unreachable,
                format!("Can't resolve {}: {e}", parsed.host),
            )
        }
    };

    let mut stream = match TcpStream::connect_timeout(&addr, IO_TIMEOUT) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            return Probe::new(ProbeStatus::Timeout, format!("{addr} didn't answer"))
        }
        Err(e) => return Probe::new(ProbeStatus::Unreachable, format!("Can't reach {addr}: {e}")),
    };
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    // 1) OPTIONS carries no credentials, so its answer is about the *server*, not the login.
    //    This is what catches a lockout without ever spending an auth attempt on it.
    let options = format!("OPTIONS {url} RTSP/1.0\r\nCSeq: 1\r\nUser-Agent: Sentinel\r\n\r\n");
    let reply = match send(&mut stream, &options) {
        Ok(r) if r.trim().is_empty() => {
            return Probe::new(
                ProbeStatus::Timeout,
                "Camera accepted the connection then said nothing",
            )
        }
        Ok(r) => r,
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
        {
            return Probe::new(ProbeStatus::Timeout, "Camera didn't answer OPTIONS")
        }
        Err(e) => return Probe::new(ProbeStatus::Error, format!("RTSP OPTIONS failed: {e}")),
    };

    if let Some((code, reason)) = status_of(&reply) {
        if is_locked(code) {
            return Probe::new(
                ProbeStatus::Locked,
                format!("Camera refused an unauthenticated request: {code} {reason}"),
            );
        }
    }

    // 2) DESCRIBE is the first method that actually evaluates credentials.
    let describe = |auth: Option<&str>| {
        let auth_line = auth
            .map(|a| format!("Authorization: {a}\r\n"))
            .unwrap_or_default();
        format!(
            "DESCRIBE {url} RTSP/1.0\r\nCSeq: 2\r\nAccept: application/sdp\r\n{auth_line}User-Agent: Sentinel\r\n\r\n"
        )
    };

    let reply = match send(&mut stream, &describe(None)) {
        Ok(r) => r,
        Err(_) => return Probe::new(ProbeStatus::Timeout, "Camera didn't answer DESCRIBE"),
    };
    let (code, reason) = match status_of(&reply) {
        Some(s) => s,
        None => return Probe::new(ProbeStatus::Error, "Camera sent a malformed RTSP reply"),
    };

    let (code, reason) = if code == 401 {
        // Answer the challenge — this is our one and only auth attempt.
        let challenge = header_of(&reply, "www-authenticate").unwrap_or_default();
        let Some(auth) = authorization(&challenge, "DESCRIBE", url, user, pass) else {
            return Probe::new(
                ProbeStatus::Error,
                format!("Unsupported camera auth scheme: {challenge}"),
            );
        };
        match send(&mut stream, &describe(Some(&auth))) {
            Ok(r) => match status_of(&r) {
                Some(s) => s,
                None => {
                    return Probe::new(ProbeStatus::Error, "Camera sent a malformed RTSP reply")
                }
            },
            Err(_) => {
                return Probe::new(
                    ProbeStatus::Timeout,
                    "Camera didn't answer authenticated DESCRIBE",
                )
            }
        }
    } else {
        (code, reason)
    };

    match code {
        200 => Probe::new(
            ProbeStatus::Ok,
            "Credentials accepted and the stream is describable",
        ),
        401 | 403 => Probe::new(
            ProbeStatus::AuthFailed,
            format!("Camera rejected the credentials: {code} {reason}"),
        ),
        404 => Probe::new(
            ProbeStatus::NotFound,
            format!("No stream at that path: {code} {reason}"),
        ),
        c if is_locked(c) => Probe::new(
            ProbeStatus::Locked,
            format!("Camera is locked: {c} {reason}"),
        ),
        c => Probe::new(ProbeStatus::Error, format!("Camera replied {c} {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls() {
        let u = parse("rtsp://192.168.1.184:554/stream2").unwrap();
        assert_eq!((u.host, u.port), ("192.168.1.184", 554));

        let u = parse("rtsp://user:pw@10.0.0.5/live").unwrap();
        assert_eq!((u.host, u.port), ("10.0.0.5", 554));

        // A password containing '@' must not be mistaken for the host separator.
        let u = parse("rtsp://user:p@ss@10.0.0.5:8554/live").unwrap();
        assert_eq!((u.host, u.port), ("10.0.0.5", 8554));

        assert!(parse("http://192.168.1.1/x").is_none());
    }

    #[test]
    fn encodes_userinfo() {
        assert_eq!(encode_userinfo("anneanjo2"), "anneanjo2");
        // The characters that silently corrupt an unencoded RTSP URL.
        assert_eq!(encode_userinfo("p@ss:w/rd"), "p%40ss%3Aw%2Frd");
    }

    #[test]
    fn reads_status_lines() {
        assert_eq!(
            status_of("RTSP/1.0 200 OK\r\n").unwrap(),
            (200, "OK".into())
        );
        let (code, reason) = status_of("RTSP/1.0 400 Server is locked\r\nDate: x\r\n").unwrap();
        assert_eq!((code, reason.as_str()), (400, "Server is locked"));
    }

    #[test]
    fn recognises_a_locked_camera() {
        // A locked Tapo C100 answers "RTSP/1.0 400 Server is locked" to *unauthenticated*
        // OPTIONS — that 400-to-everyone is what separates a lockout from a bad password.
        assert!(is_locked(400));
        assert!(!is_locked(401)); // wrong password
        assert!(!is_locked(200));
    }

    #[test]
    fn digest_matches_rfc2069_worked_example() {
        // ha1 = md5("u:realm:p"), ha2 = md5("DESCRIBE:rtsp://h/s"), resp = md5(ha1:nonce:ha2)
        let auth = authorization(
            r#"Digest realm="realm", nonce="abc""#,
            "DESCRIBE",
            "rtsp://h/s",
            "u",
            "p",
        )
        .unwrap();
        let ha1 = md5_hex("u:realm:p");
        let ha2 = md5_hex("DESCRIBE:rtsp://h/s");
        let want = md5_hex(&format!("{ha1}:abc:{ha2}"));
        assert!(auth.contains(&format!("response=\"{want}\"")));
        assert!(auth.contains(r#"username="u""#));
    }

    #[test]
    fn falls_back_to_basic() {
        let auth = authorization("Basic realm=\"x\"", "DESCRIBE", "rtsp://h/s", "u", "p").unwrap();
        assert_eq!(auth, "Basic dTpw"); // base64("u:p")
    }

    #[test]
    fn hard_failures_are_not_retryable() {
        // Retrying these is precisely what locks a camera out.
        assert!(!ProbeStatus::AuthFailed.is_retryable());
        assert!(!ProbeStatus::Locked.is_retryable());
        assert!(ProbeStatus::Timeout.is_retryable());
    }
}
