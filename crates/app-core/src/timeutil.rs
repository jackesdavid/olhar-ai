//! Shared RFC3339 time helpers. Timestamps across the platform are stored as RFC3339 **text**
//! (sqlx is built without the timestamptz decoder), always in the local offset, so text comparison
//! of two values from the same source is correct. These used to live in the `alerts` module; they
//! moved here so they outlive it.

/// Current local time as an RFC3339 string — the stored timestamp format.
pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339()
}

/// An RFC3339 timestamp `secs` into the future (or the past, if negative) — for expiries.
///
/// Produced from the same `chrono::Local` clock as [`now_rfc3339`], and that matters: expiry checks
/// compare these as **text** (`expires_at > now`), which is only correct when both sides carry the
/// same UTC offset. Never mix a value from here with one built in a different zone.
pub fn plus_secs_rfc3339(secs: i64) -> String {
    (chrono::Local::now() + chrono::Duration::seconds(secs)).to_rfc3339()
}

/// Whether an RFC3339 timestamp falls on today's local date.
pub fn is_today(created_at: &str) -> bool {
    let today = chrono::Local::now().date_naive();
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.with_timezone(&chrono::Local).date_naive() == today)
        .unwrap_or(false)
}

/// Whole seconds from `from` to `to`, both RFC3339. `None` if either fails to parse.
///
/// Parses rather than comparing text: elapsed time is arithmetic, and the text-comparison shortcut
/// the rest of this module relies on only answers "which is later". Negative results are clamped to
/// zero — a clock adjustment between two writes should read as "just now", never as a negative
/// duration that a downstream `>` comparison would silently pass.
pub fn secs_between(from: &str, to: &str) -> Option<i64> {
    let a = chrono::DateTime::parse_from_rfc3339(from).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(to).ok()?;
    Some((b - a).num_seconds().max(0))
}

/// An RFC3339 timestamp `secs_ago` seconds in the past.
///
/// For reporting when something last happened from an elapsed duration — the frame hub knows "42s
/// since the last frame", while the stored value has to be an absolute time so it stays meaningful
/// after the report is written.
pub fn now_rfc3339_minus(secs_ago: i64) -> String {
    (chrono::Local::now() - chrono::Duration::seconds(secs_ago)).to_rfc3339()
}
