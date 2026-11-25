//! Calendar math. Message timestamps are Unix seconds (UTC); a clock or a date
//! shown to the reader is first moved to local time: by the system zone in the
//! real app, by a fixed offset (zero by default) in the demo and in tests, so
//! their output never depends on the machine they run on.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

static SYSTEM_ZONE: AtomicBool = AtomicBool::new(false);
static FIXED_OFFSET: AtomicI64 = AtomicI64::new(0);

/// Shows times in the system's time zone (the real app).
pub fn use_system_zone() {
    SYSTEM_ZONE.store(true, Ordering::Relaxed);
}

/// Shows times at a fixed offset from UTC, in seconds (the demo, tests).
pub fn use_fixed_offset(seconds: i64) {
    SYSTEM_ZONE.store(false, Ordering::Relaxed);
    FIXED_OFFSET.store(seconds, Ordering::Relaxed);
}

static NOW_OVERRIDE: AtomicI64 = AtomicI64::new(i64::MIN);

/// Pins "now" (the demo and tests: the chat list's `Ontem` depends on it).
pub fn set_now(ts: i64) {
    NOW_OVERRIDE.store(ts, Ordering::Relaxed);
}

/// The current Unix time, or the pinned one.
pub fn now() -> i64 {
    match NOW_OVERRIDE.load(Ordering::Relaxed) {
        i64::MIN => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64),
        pinned => pinned,
    }
}

/// `ts` moved to local wall-clock time.
pub fn local(ts: i64) -> i64 {
    if SYSTEM_ZONE.load(Ordering::Relaxed)
        && let Ok(stamp) = jiff::Timestamp::from_second(ts)
    {
        return ts + i64::from(jiff::tz::TimeZone::system().to_offset(stamp).seconds());
    }
    ts + FIXED_OFFSET.load(Ordering::Relaxed)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`]: `(year, month, day)`.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub const SECONDS_PER_DAY: i64 = 86_400;

/// The day number (days since the epoch) of a timestamp.
pub fn day_of(ts: i64) -> i64 {
    local(ts).div_euclid(SECONDS_PER_DAY)
}

/// `HH:MM`, zero padded.
pub fn clock(ts: i64) -> String {
    let secs = local(ts).rem_euclid(SECONDS_PER_DAY);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// `D/M/YYYY`, not padded, as WhatsApp writes dates in its text exports.
pub fn short_date(ts: i64) -> String {
    let (y, m, d) = civil_from_days(day_of(ts));
    format!("{d}/{m}/{y}")
}

const MONTHS: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// The label of a day separator, such as `12 de março de 2026`.
pub fn long_date(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{d} de {} de {y}", MONTHS[(m - 1) as usize])
}

const WEEKDAYS: [&str; 7] = [
    "domingo",
    "segunda-feira",
    "terça-feira",
    "quarta-feira",
    "quinta-feira",
    "sexta-feira",
    "sábado",
];

/// The time column of a chat row, as WhatsApp writes it: the clock today,
/// `Ontem`, the weekday within the week, otherwise the date.
pub fn list_label(ts: i64, now: i64) -> String {
    let (day, today) = (day_of(ts), day_of(now));
    match today - day {
        ..=0 => clock(ts),
        1 => "Ontem".to_owned(),
        2..=6 => WEEKDAYS[(day + 4).rem_euclid(7) as usize].to_owned(),
        _ => short_date(ts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for days in [-800_000, -1, 0, 1, 11_016, 20_000, 60_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn formats_like_whatsapp() {
        let ts = days_from_civil(2026, 3, 5) * SECONDS_PER_DAY + 9 * 3600 + 7 * 60 + 33;
        assert_eq!(clock(ts), "09:07");
        assert_eq!(short_date(ts), "5/3/2026");
        assert_eq!(long_date(day_of(ts)), "5 de março de 2026");
    }

    #[test]
    fn the_chat_list_time_follows_the_day() {
        let at = |d: u32, h: i64| days_from_civil(2026, 9, d) * SECONDS_PER_DAY + h * 3600 + 5 * 60;
        let now = at(10, 15);
        assert_eq!(list_label(at(10, 9), now), "09:05");
        assert_eq!(list_label(at(9, 23), now), "Ontem");
        // 2026-09-07 is a Monday.
        assert_eq!(list_label(at(7, 8), now), "segunda-feira");
        assert_eq!(list_label(at(1, 8), now), "1/9/2026");
    }
}
