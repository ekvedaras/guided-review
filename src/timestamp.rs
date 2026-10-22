use std::fmt;

/// Seconds since the epoch, shown as `2026-10-22T13-05-09` (file-name safe).
pub struct UtcTimestamp(u64);

impl UtcTimestamp {
    pub fn from_epoch_seconds(seconds: u64) -> Self {
        Self(seconds)
    }

    pub fn now() -> Self {
        Self::from_epoch_seconds(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        )
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.0;
        let (days, time) = ((seconds / 86_400) as i64, seconds % 86_400);
        // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}",
            time / 3600,
            time % 3600 / 60,
            time % 60
        )
    }
}
