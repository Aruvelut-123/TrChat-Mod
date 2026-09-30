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

/// The instant the plugin was loaded, for [`uptime_seconds`].
///
/// WASI only exposes a monotonic clock, so an uptime needs a reference instant;
/// [`mark_start`] records it while the host loads the plugin, which happens
/// during server startup.
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Records the plugin-load instant (idempotent). Called from `on_load`.
pub fn mark_start() {
    let _ = START.set(std::time::Instant::now());
}

/// Seconds since [`mark_start`] — the port's stand-in for the JVM uptime the Mod
/// reports for `%server_uptime%` (`PlaceholderResolver.java:128`).
///
/// The Mod measures the whole JVM; the sandbox can only measure the plugin, and
/// a plugin that was never marked (unit tests) counts from zero.
pub fn uptime_seconds() -> i64 {
    START
        .get()
        .map(|start| start.elapsed().as_secs() as i64)
        .unwrap_or(0)
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

/// `(year, month, day)` → days since 1970-01-01, the inverse of
/// [`civil_from_days`] (Howard Hinnant's `days_from_civil`).
///
/// The caller validates the ranges; a nonsense month (0 or 13) still maps to a
/// definite day count rather than panicking, because the countdown parser rejects
/// such values before calling this.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    // January/February belong to the previous, March-based year.
    let shifted_year = year - i64::from(month <= 2);
    let era = if shifted_year >= 0 {
        shifted_year
    } else {
        shifted_year - 399
    } / 400;
    let year_of_era = (shifted_year - era * 400) as u64; // [0, 399]
    let march_month = if month > 2 { month - 3 } else { month + 9 } as u64; // [0, 11]
    let day_of_year = (153 * march_month + 2) / 5 + u64::from(day) - 1; // [0, 365]
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era as i64 - 719_468
}

/// Renders an instant with the subset of `java.time.format.DateTimeFormatter`
/// patterns TrChat's configs use.
///
/// Upstream `%server_time_<pattern>%` is
/// `ZonedDateTime.now().format(DateTimeFormatter.ofPattern(pattern))`; an
/// illegal pattern makes the formatter throw and the resolver returns `""`. WASI
/// exposes no time zone database, so the instant is rendered in **UTC**
/// (documented deviation — the Mod uses the server's system zone).
///
/// Supported letters (case-sensitive, as in Java): `y`/`yy`/`yyyy` years,
/// `M`/`MM`/`MMM`/`MMMM` months (numeric/short/full English), `d`/`dd` days,
/// `H`/`HH` 24-hour, `h`/`hh` 12-hour, `m`/`mm` minutes, `s`/`ss` seconds.
/// `'` quotes literal text (`''` is a single quote). Any other ASCII letter is
/// an illegal pattern → `""`.
///
/// Note that the placeholder resolver lowercases the whole token before routing
/// it here (§1.1 step 3), so `%server_time_HH:mm:ss%` arrives as `hh:mm:ss` —
/// the 12-hour clock. That is the upstream behaviour, not a bug here.
pub fn format_pattern(pattern: &str, millis: i64) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let seconds = millis.div_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour24 = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let second = time_of_day % 60;
    let hour12 = if hour24 % 12 == 0 { 12 } else { hour24 % 12 };

    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                // `''` inside a quoted section is a literal single quote that
                // keeps the section open (Java rule).
                if chars[i] == '\'' && chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                if chars[i] == '\'' {
                    closed = true;
                    i += 1;
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            // Java rejects an unterminated quote.
            if !closed {
                return String::new();
            }
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut end = i;
        while end < chars.len() && chars[end] == c {
            end += 1;
        }
        let run = end - i;
        let piece = match c {
            'y' => pad(year, run),
            'M' => match run {
                1 => month.to_string(),
                2 => format!("{month:02}"),
                3 => MONTHS[(month - 1) as usize][..3].to_string(),
                4 => MONTHS[(month - 1) as usize].to_string(),
                // 5+ letters are the narrow form, which the port does not ship.
                _ => return String::new(),
            },
            'd' => pad(i64::from(day), run),
            'H' => pad(hour24, run),
            'h' => pad(hour12, run),
            'm' => pad(minute, run),
            's' => pad(second, run),
            _ => return String::new(),
        };
        out.push_str(&piece);
        i = end;
    }
    out
}

/// Zero-pads `value` to at least `width` digits; a single letter means "no
/// padding", which is Java's minimum-width rule.
fn pad(value: i64, width: usize) -> String {
    if width <= 1 {
        value.to_string()
    } else {
        format!("{value:0>width$}")
    }
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, format_millis, format_pattern, now_hhmmss};

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

    /// §1.4 — the pattern letters the shipped configs use. The instant is
    /// 2024-03-01 13:05:09 UTC.
    #[test]
    fn patterns_render_the_instant() {
        let millis = 1_709_298_309_000; // 2024-03-01 13:05:09 UTC
        assert_eq!(format_millis(millis), "2024-03-01 13:05:09");
        assert_eq!(
            format_pattern("yyyy-MM-dd HH:mm:ss", millis),
            "2024-03-01 13:05:09"
        );
        // Lowercased, as the resolver delivers it: `mm` is a *minute*.
        assert_eq!(format_pattern("hh:mm:ss", millis), "01:05:09");
        assert_eq!(format_pattern("yyyy-mm-dd", millis), "2024-05-01");
        // Single letters are the unpadded numeric forms.
        assert_eq!(format_pattern("y-M-d H:m:s", millis), "2024-3-1 13:5:9");
        // Text months and the 12-hour clock (0:xx and 12:xx both map to 12).
        assert_eq!(format_pattern("d MMMM yyyy", millis), "1 March 2024");
        assert_eq!(format_pattern("d MMM yyyy", millis), "1 Mar 2024");
        assert_eq!(format_pattern("h", 1_709_251_200_000), "12");
        assert_eq!(format_pattern("H", 1_709_251_200_000), "0");
    }

    /// Literals, quoting, and the illegal-pattern path (Java throws, the
    /// upstream resolver swallows it and yields `""`).
    #[test]
    fn patterns_handle_literals_and_reject_illegal_letters() {
        let millis = 1_709_298_309_000; // 2024-03-01 13:05:09 UTC
                                        // Non-letters pass through, including a quoted letter run.
        assert_eq!(format_pattern("HH:mm 'o''clock'", millis), "13:05 o'clock");
        assert_eq!(format_pattern("yyyy/MM/dd", millis), "2024/03/01");
        // An unsupported letter, an unterminated quote and the narrow form.
        assert_eq!(format_pattern("yyyy-MM-dd E", millis), "");
        assert_eq!(format_pattern("HH:mm 'oops", millis), "");
        assert_eq!(format_pattern("MMMMM", millis), "");
        // An empty pattern renders empty rather than falling back to anything.
        assert_eq!(format_pattern("", millis), "");
    }
}
