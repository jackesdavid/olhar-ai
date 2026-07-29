//! Next-occurrence math for scheduled workflow triggers (#111). Pure + timezone-aware (DST-correct
//! via chrono-tz), minute granularity. Given a [`Schedule`] + "now" (UTC), returns the next UTC
//! instant the graph should fire, **strictly after** `from` (so re-computing right after a fire
//! advances to the following occurrence).
use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;

use super::domain::{Schedule, ScheduleFreq};

/// The next instant (UTC) this schedule fires, strictly after `from`. `None` if the schedule is
/// malformed (bad tz, no valid times/days) or nothing matches within a sane horizon.
pub fn next_occurrence(sched: &Schedule, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let tz: Tz = if sched.tz.trim().is_empty() {
        chrono_tz::UTC
    } else {
        sched.tz.parse().ok()?
    };
    match sched.freq {
        ScheduleFreq::Hourly => next_hourly(sched.minute, tz, from),
        _ => next_calendar(sched, tz, from),
    }
}

/// Every hour at `minute`, in `tz`.
fn next_hourly(minute: u8, tz: Tz, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let minute = (minute as u32).min(59);
    let from_local = from.with_timezone(&tz);
    let mut dt = from_local
        .date_naive()
        .and_hms_opt(from_local.hour(), minute, 0)?;
    for _ in 0..48 {
        if let Some(utc) = local_to_utc(tz, dt) {
            if utc > from {
                return Some(utc);
            }
        }
        dt += Duration::hours(1);
    }
    None
}

/// Daily / Weekly / Monthly / Yearly: scan forward day by day, and on a matching day try each
/// configured time-of-day.
fn next_calendar(sched: &Schedule, tz: Tz, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut times = parse_times(&sched.times);
    if times.is_empty() {
        return None;
    }
    times.sort_unstable();

    let horizon = match sched.freq {
        ScheduleFreq::Yearly => 366 * 2 + 2,
        ScheduleFreq::Monthly => 62,
        _ => 8,
    };
    let mut date = from.with_timezone(&tz).date_naive();
    for _ in 0..horizon {
        if day_matches(sched, date) {
            for &(h, m) in &times {
                if let Some(local) = date.and_hms_opt(h, m, 0) {
                    if let Some(utc) = local_to_utc(tz, local) {
                        if utc > from {
                            return Some(utc);
                        }
                    }
                }
            }
        }
        date = date.succ_opt()?;
    }
    None
}

fn day_matches(sched: &Schedule, date: NaiveDate) -> bool {
    match sched.freq {
        ScheduleFreq::Daily => true,
        ScheduleFreq::Weekly => {
            let wd = date.weekday().num_days_from_monday() as u8; // 0 = Monday
            sched.weekdays.contains(&wd)
        }
        ScheduleFreq::Monthly => day_of_month_matches(date, sched.day_of_month),
        ScheduleFreq::Yearly => {
            date.month() == sched.month as u32 && day_of_month_matches(date, sched.day_of_month)
        }
        ScheduleFreq::Hourly => true,
    }
}

/// True if `date` is the schedule's day-of-month, clamped to the month's length — so a "monthly on
/// the 31st" schedule still fires (on the last day) in shorter months like February.
fn day_of_month_matches(date: NaiveDate, dom: u8) -> bool {
    let dom = (dom as u32).clamp(1, 31);
    let last = last_day_of_month(date.year(), date.month());
    date.day() == dom.min(last)
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

fn parse_times(times: &[String]) -> Vec<(u32, u32)> {
    times
        .iter()
        .filter_map(|t| {
            let (h, m) = t.split_once(':')?;
            let h: u32 = h.trim().parse().ok()?;
            let m: u32 = m.trim().parse().ok()?;
            (h < 24 && m < 60).then_some((h, m))
        })
        .collect()
}

/// Resolve a local wall-clock time to UTC, handling DST: a nonexistent local time (spring-forward
/// gap) yields `None` (that occurrence is skipped); an ambiguous one (fall-back overlap) takes the
/// earliest instant.
fn local_to_utc(tz: Tz, local: NaiveDateTime) -> Option<DateTime<Utc>> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => Some(earliest.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(freq: ScheduleFreq) -> Schedule {
        Schedule {
            freq,
            tz: String::new(),
            times: vec![],
            minute: 0,
            weekdays: vec![],
            day_of_month: 0,
            month: 0,
        }
    }
    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn daily_at_time_utc() {
        let s = Schedule {
            times: vec!["09:00".into()],
            ..sched(ScheduleFreq::Daily)
        };
        // before 9am → today 9am
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T06:00:00Z")).unwrap(),
            utc("2026-07-22T09:00:00Z")
        );
        // after 9am → tomorrow 9am
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T10:00:00Z")).unwrap(),
            utc("2026-07-23T09:00:00Z")
        );
    }

    #[test]
    fn n_times_a_day_picks_next_slot() {
        let s = Schedule {
            times: vec!["08:00".into(), "12:00".into(), "18:00".into()],
            ..sched(ScheduleFreq::Daily)
        };
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T09:00:00Z")).unwrap(),
            utc("2026-07-22T12:00:00Z")
        );
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T19:00:00Z")).unwrap(),
            utc("2026-07-23T08:00:00Z")
        );
    }

    #[test]
    fn timezone_sao_paulo_daily_9am() {
        // 09:00 America/Sao_Paulo (UTC-3, no DST since 2019) = 12:00 UTC.
        let s = Schedule {
            tz: "America/Sao_Paulo".into(),
            times: vec!["09:00".into()],
            ..sched(ScheduleFreq::Daily)
        };
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T00:00:00Z")).unwrap(),
            utc("2026-07-22T12:00:00Z")
        );
    }

    #[test]
    fn weekly_mon_wed() {
        // 2026-07-22 is a Wednesday. weekdays 0=Mon, 2=Wed. At 09:00.
        let s = Schedule {
            weekdays: vec![0, 2],
            times: vec!["09:00".into()],
            ..sched(ScheduleFreq::Weekly)
        };
        // Wed 10:00 → next is next Mon (2026-07-27) 09:00
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T10:00:00Z")).unwrap(),
            utc("2026-07-27T09:00:00Z")
        );
    }

    #[test]
    fn monthly_day_15() {
        let s = Schedule {
            day_of_month: 15,
            times: vec!["00:00".into()],
            ..sched(ScheduleFreq::Monthly)
        };
        assert_eq!(
            next_occurrence(&s, utc("2026-07-20T00:00:00Z")).unwrap(),
            utc("2026-08-15T00:00:00Z")
        );
    }

    #[test]
    fn monthly_day_31_clamps_to_feb() {
        let s = Schedule {
            day_of_month: 31,
            times: vec!["00:00".into()],
            ..sched(ScheduleFreq::Monthly)
        };
        // From Feb 1 2027 → fires Feb 28 2027 (clamped).
        assert_eq!(
            next_occurrence(&s, utc("2027-02-01T00:00:00Z")).unwrap(),
            utc("2027-02-28T00:00:00Z")
        );
    }

    #[test]
    fn hourly_at_minute() {
        let s = Schedule {
            minute: 30,
            ..sched(ScheduleFreq::Hourly)
        };
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T10:15:00Z")).unwrap(),
            utc("2026-07-22T10:30:00Z")
        );
        assert_eq!(
            next_occurrence(&s, utc("2026-07-22T10:45:00Z")).unwrap(),
            utc("2026-07-22T11:30:00Z")
        );
    }

    #[test]
    fn empty_times_is_none() {
        assert!(
            next_occurrence(&sched(ScheduleFreq::Daily), utc("2026-07-22T00:00:00Z")).is_none()
        );
    }
}
