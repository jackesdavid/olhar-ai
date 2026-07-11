//! Shared RFC3339 time helpers. Timestamps across the platform are stored as RFC3339 **text**
//! (sqlx is built without the timestamptz decoder), always in the local offset, so text comparison
//! of two values from the same source is correct. These used to live in the `alerts` module; they
//! moved here so they outlive it.

/// Current local time as an RFC3339 string — the stored timestamp format.
pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339()
}

/// Whether an RFC3339 timestamp falls on today's local date.
pub fn is_today(created_at: &str) -> bool {
    let today = chrono::Local::now().date_naive();
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.with_timezone(&chrono::Local).date_naive() == today)
        .unwrap_or(false)
}
