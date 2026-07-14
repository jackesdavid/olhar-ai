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
