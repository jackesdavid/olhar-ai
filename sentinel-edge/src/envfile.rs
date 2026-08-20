//! The edge reads its own `edge.env` (card #81).
//!
//! It used to rely on the service manager to inject the environment, and the two platforms did it
//! differently: systemd points `EnvironmentFile=` at `/etc/sentinel-edge/edge.env` and re-reads it
//! on every start, but **launchd copies the values into the plist at install time**. So on macOS a
//! file the edge writes at runtime — which is exactly what enrollment does — would never reach the
//! process.
//!
//! Loading the file ourselves makes the two behave the same, and means a box can acquire its
//! identity while running and simply restart into it. It also spares the Windows service (#83) from
//! having to plumb an environment at all.
//!
//! An already-set variable **wins** over the file, so `CONTROL_URL=… cargo run` still overrides in
//! dev, and so an operator's explicit override isn't silently undone by a stale file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

/// Where the box's identity lives. `install.sh` writes it; enrollment rewrites it.
pub fn path() -> PathBuf {
    std::env::var("EDGE_ENV_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::paths::env_file())
}

/// Parse `KEY=VALUE` lines (blank lines and `#` comments ignored) into the process environment,
/// without clobbering anything already set. Missing file = nothing to do, not an error: a
/// dev edge run from a checkout has no `/etc/sentinel-edge`.
pub fn load() {
    let p = path();
    let Ok(raw) = std::fs::read_to_string(&p) else {
        return;
    };
    let mut loaded = 0usize;
    for (k, v) in parse(&raw) {
        if std::env::var_os(&k).is_none() {
            std::env::set_var(&k, &v);
            loaded += 1;
        }
    }
    if loaded > 0 {
        log::info!("loaded {loaded} setting(s) from {}", p.display());
    }
}

/// Merge `updates` into the file, preserving every other line's key, then write it back with
/// owner-only permissions — it holds the box token.
pub fn merge_write(updates: &[(&str, &str)]) -> std::io::Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existing = std::fs::read_to_string(&p).unwrap_or_default();
    let mut map: BTreeMap<String, String> = parse(&existing).into_iter().collect();
    for (k, v) in updates {
        map.insert((*k).to_string(), (*v).to_string());
    }

    // Write via a temp file + rename so a crash mid-write can't leave the box with a truncated
    // identity — the one file it cannot afford to lose.
    let tmp = p.with_extension("env.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        for (k, v) in &map {
            writeln!(f, "{k}={v}")?;
        }
        f.sync_all()?;
    }
    set_owner_only(&tmp)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only(p: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only(_p: &std::path::Path) -> std::io::Result<()> {
    Ok(()) // Windows inherits the ACL of %ProgramData%\Sentinel — see card #80.
}

fn parse(raw: &str) -> Vec<(String, String)> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_skips_comments_and_blanks() {
        let got = parse("# a comment\n\nCONTROL_URL=https://x\nBOX_TOKEN = sbx_1  \n");
        assert_eq!(
            got,
            vec![
                ("CONTROL_URL".to_string(), "https://x".to_string()),
                ("BOX_TOKEN".to_string(), "sbx_1".to_string()),
            ]
        );
    }

    /// A value can legitimately contain `=` (a base64-ish key). Splitting on the FIRST `=` only.
    #[test]
    fn keeps_equals_signs_inside_a_value() {
        let got = parse("TS_AUTHKEY=tskey-auth-abc=def=");
        assert_eq!(
            got,
            vec![("TS_AUTHKEY".to_string(), "tskey-auth-abc=def=".to_string())]
        );
    }
}
