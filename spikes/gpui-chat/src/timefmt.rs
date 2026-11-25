//! Calendar math without a time zone database: the synthetic timestamps are
//! "local wall-clock seconds since 1970-01-01", so there is nothing to convert.

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
    ts.div_euclid(SECONDS_PER_DAY)
}

/// `HH:MM`, zero padded.
pub fn clock(ts: i64) -> String {
    let secs = ts.rem_euclid(SECONDS_PER_DAY);
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
}
