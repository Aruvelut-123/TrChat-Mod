//! Wall-clock helpers — the port's substitute for `System.currentTimeMillis()`
//! and the timestamp formatting the upstream does with `SimpleDateFormat`.
//!
//! The plugin runs inside WASI, which exposes no time zone database, so every
//! timestamp here is rendered in **UTC**. The upstream formats with the server's
//! system time zone; that difference is a documented deviation (it affects the
//! console log prefix and `muteExpiry`).
//!
//! All arithmetic is plain integer maths so it stays deterministic and testable.

/// Milliseconds since the Unix epoch — the unit of `PlayerState.muteUntil`.
///
/// A clock before 1970 (or an unavailable clock) yields `0`, which
/// [`crate::playerdata::PlayerState::is_mute_active`] reads as "not muted".
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The current time as `HH:mm:ss` in UTC — the `{0}` of the console log
/// formats (spec §1.6).
pub fn now_hhmmss() -> String {
    format_millis(now_millis())
        .split(' ')
        .nth(1)
        .unwrap_or("00:00:00")
        .to_string()
}

/// Formats epoch milliseconds as `yyyy-MM-dd HH:mm:ss` in UTC.
///
/// Used for `muteExpiry` (`ModerationService.java:64-67`).
pub fn format_millis(millis: i64) -> String {
    let seconds = millis.div_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        time / 3_600,
        (time % 3_600) / 60,
        time % 60
    )
}

/// Days since 1970-01-01 → `(year, month, day)`, after Howard Hinnant's
/// `civil_from_days`. Valid for the whole `i64` range the caller can produce.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153; // [0, 11], March = 0
    let day = (day_of_year - (153 * march_month + 2) / 5 + 1) as u32;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, format_millis, now_hhmmss};

    /// The epoch itself, plus a leap day and a year boundary.
    #[test]
    fn epoch_millis_format_as_utc_dates() {
        assert_eq!(format_millis(0), "1970-01-01 00:00:00");
        // 2000-02-29 12:34:56 UTC.
        assert_eq!(format_millis(951_827_696_000), "2000-02-29 12:34:56");
        // 2024-03-01 00:00:00 UTC — the day after a leap day.
        assert_eq!(format_millis(1_709_251_200_000), "2024-03-01 00:00:00");
    }

    /// Sub-second precision is truncated, and times before the epoch floor
    /// towards the earlier day rather than towards zero.
    #[test]
    fn formatting_truncates_and_floors() {
        assert_eq!(format_millis(999), "1970-01-01 00:00:00");
        assert_eq!(format_millis(1_000), "1970-01-01 00:00:01");
        assert_eq!(format_millis(-1), "1969-12-31 23:59:59");
    }

    /// The civil-date conversion handles the proleptic range the mute model
    /// can reach, including pre-1970 values.
    #[test]
    fn civil_days_round_trip_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    /// `now_hhmmss` is always an 8-character `HH:mm:ss` clock.
    #[test]
    fn clock_reads_as_hh_mm_ss() {
        let now = now_hhmmss();
        assert_eq!(now.len(), 8, "{now}");
        assert_eq!(&now[2..3], ":");
        assert_eq!(&now[5..6], ":");
    }
}
