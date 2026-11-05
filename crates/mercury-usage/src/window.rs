//! Usage windows (PLAN.md 3.1): rolling, calendar and provider-reported.

use time::{Date, Duration, Month, OffsetDateTime, Time, UtcOffset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarPeriod {
    Daily,
    Weekly,
    Monthly,
}

/// A calendar window that resets at a fixed local time.
///
/// Time zones are fixed UTC offsets (e.g. `-03:00` for America/Sao_Paulo, which
/// has had no DST since 2019); named IANA zones would need a tz database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarWindow {
    pub period: CalendarPeriod,
    /// Monthly: day of month 1..=31 (clamped to the month's length).
    /// Weekly: ISO weekday 1 = Monday ..= 7 = Sunday. Ignored for daily.
    pub reset_day: u8,
    /// Local hour 0..=23 at which the window resets.
    pub reset_hour: u8,
    pub utc_offset: UtcOffset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageWindow {
    /// The last `minutes`, sliding continuously.
    Rolling {
        minutes: u32,
    },
    Calendar(CalendarWindow),
    /// A window owned by the provider (`five_hour`, `seven_day`, `spend_limit`,
    /// `primary`, `secondary`); its state comes from [`crate::ProviderLimit`].
    Provider {
        key: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    pub start: OffsetDateTime,
    /// Next reset. For rolling windows this is `now` (they never reset as a whole).
    pub end: OffsetDateTime,
}

impl WindowBounds {
    pub fn contains(&self, at: OffsetDateTime) -> bool {
        self.start <= at && at < self.end
    }
}

impl CalendarWindow {
    pub fn new(
        period: CalendarPeriod,
        reset_day: u8,
        reset_hour: u8,
        utc_offset: UtcOffset,
    ) -> Result<Self, String> {
        match period {
            CalendarPeriod::Monthly if !(1..=31).contains(&reset_day) => {
                return Err(format!("monthly reset_day must be 1..=31, got {reset_day}"));
            }
            CalendarPeriod::Weekly if !(1..=7).contains(&reset_day) => {
                return Err(format!(
                    "weekly reset_day must be 1 (Monday)..=7 (Sunday), got {reset_day}"
                ));
            }
            _ => {}
        }
        if reset_hour > 23 {
            return Err(format!("reset_hour must be 0..=23, got {reset_hour}"));
        }
        Ok(Self {
            period,
            reset_day,
            reset_hour,
            utc_offset,
        })
    }

    fn reset_time(&self) -> Time {
        Time::from_hms(self.reset_hour.min(23), 0, 0).unwrap_or(Time::MIDNIGHT)
    }

    fn at(&self, date: Date) -> OffsetDateTime {
        date.with_time(self.reset_time())
            .assume_offset(self.utc_offset)
    }

    fn monthly_reset(&self, year: i32, month: Month) -> Option<OffsetDateTime> {
        let day = self.reset_day.clamp(1, month.length(year));
        Date::from_calendar_date(year, month, day)
            .ok()
            .map(|date| self.at(date))
    }

    pub fn bounds(&self, now: OffsetDateTime) -> Option<WindowBounds> {
        let local = now.to_offset(self.utc_offset);
        let today = local.date();
        match self.period {
            CalendarPeriod::Daily => {
                let mut start = self.at(today);
                if now < start {
                    start = self.at(today.previous_day()?);
                }
                Some(WindowBounds {
                    start,
                    end: start + Duration::days(1),
                })
            }
            CalendarPeriod::Weekly => {
                let target = self.reset_day.clamp(1, 7) - 1;
                let current = today.weekday().number_days_from_monday();
                let days_back = (current + 7 - target) % 7;
                let mut start = self.at(today.checked_sub(Duration::days(i64::from(days_back)))?);
                if now < start {
                    start -= Duration::days(7);
                }
                Some(WindowBounds {
                    start,
                    end: start + Duration::days(7),
                })
            }
            CalendarPeriod::Monthly => {
                let (year, month) = (today.year(), today.month());
                let this_month = self.monthly_reset(year, month)?;
                let (start, end) = if now >= this_month {
                    let (next_year, next_month) = next_month(year, month);
                    (this_month, self.monthly_reset(next_year, next_month)?)
                } else {
                    let (prev_year, prev_month) = previous_month(year, month);
                    (self.monthly_reset(prev_year, prev_month)?, this_month)
                };
                Some(WindowBounds { start, end })
            }
        }
    }
}

fn next_month(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::December => (year + 1, Month::January),
        month => (year, month.next()),
    }
}

fn previous_month(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::January => (year - 1, Month::December),
        month => (year, month.previous()),
    }
}

impl UsageWindow {
    /// Time range the window covers at `now`. `None` for provider windows,
    /// whose range is whatever the provider reports via `resets_at`.
    pub fn bounds(&self, now: OffsetDateTime) -> Option<WindowBounds> {
        match self {
            Self::Rolling { minutes } => Some(WindowBounds {
                start: now - Duration::minutes(i64::from(*minutes)),
                end: now,
            }),
            Self::Calendar(calendar) => calendar.bounds(now),
            Self::Provider { .. } => None,
        }
    }

    /// When the window next resets as a whole (calendar only); used to schedule
    /// recomputation at the boundary instead of polling.
    pub fn next_reset(&self, now: OffsetDateTime) -> Option<OffsetDateTime> {
        match self {
            Self::Calendar(calendar) => calendar.bounds(now).map(|bounds| bounds.end),
            Self::Rolling { .. } | Self::Provider { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(value: &str) -> OffsetDateTime {
        crate::parse_timestamp(value).unwrap()
    }

    fn brt() -> UtcOffset {
        UtcOffset::from_hms(-3, 0, 0).unwrap()
    }

    #[test]
    fn monthly_window_resets_on_local_day() {
        let window = CalendarWindow::new(CalendarPeriod::Monthly, 1, 0, brt()).unwrap();
        // 2026-10-01 01:00Z is still Sep 30 22:00 in -03:00.
        let bounds = window.bounds(ts("2026-10-01T01:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2026-09-01T03:00:00Z"));
        assert_eq!(bounds.end, ts("2026-10-01T03:00:00Z"));
        let bounds = window.bounds(ts("2026-10-01T03:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2026-10-01T03:00:00Z"));
        assert_eq!(bounds.end, ts("2026-11-01T03:00:00Z"));
    }

    #[test]
    fn monthly_reset_day_is_clamped_and_wraps_years() {
        let window = CalendarWindow::new(CalendarPeriod::Monthly, 31, 0, UtcOffset::UTC).unwrap();
        let bounds = window.bounds(ts("2027-02-15T00:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2027-01-31T00:00:00Z"));
        assert_eq!(bounds.end, ts("2027-02-28T00:00:00Z"));
        let bounds = window.bounds(ts("2026-12-31T12:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2026-12-31T00:00:00Z"));
        assert_eq!(bounds.end, ts("2027-01-31T00:00:00Z"));
    }

    #[test]
    fn weekly_and_daily_windows() {
        // 2026-10-05 is a Monday.
        let weekly = CalendarWindow::new(CalendarPeriod::Weekly, 1, 9, UtcOffset::UTC).unwrap();
        let bounds = weekly.bounds(ts("2026-10-05T08:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2026-09-28T09:00:00Z"));
        assert_eq!(bounds.end, ts("2026-10-05T09:00:00Z"));
        let bounds = weekly.bounds(ts("2026-10-07T00:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2026-10-05T09:00:00Z"));

        let sunday = CalendarWindow::new(CalendarPeriod::Weekly, 7, 0, UtcOffset::UTC).unwrap();
        let bounds = sunday.bounds(ts("2026-10-05T08:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2026-10-04T00:00:00Z"));

        let daily = CalendarWindow::new(CalendarPeriod::Daily, 1, 6, brt()).unwrap();
        let bounds = daily.bounds(ts("2026-10-05T08:00:00Z")).unwrap();
        assert_eq!(bounds.start, ts("2026-10-04T09:00:00Z"));
        assert_eq!(bounds.end, ts("2026-10-05T09:00:00Z"));
        assert!(bounds.contains(ts("2026-10-05T08:59:59Z")));
        assert!(!bounds.contains(ts("2026-10-05T09:00:00Z")));
    }

    #[test]
    fn rolling_and_provider_windows() {
        let now = ts("2026-10-05T12:00:00Z");
        let rolling = UsageWindow::Rolling { minutes: 180 };
        assert_eq!(
            rolling.bounds(now).unwrap().start,
            ts("2026-10-05T09:00:00Z")
        );
        assert_eq!(rolling.next_reset(now), None);
        let provider = UsageWindow::Provider {
            key: "five_hour".into(),
        };
        assert_eq!(provider.bounds(now), None);
    }

    #[test]
    fn rejects_invalid_calendar_parameters() {
        assert!(CalendarWindow::new(CalendarPeriod::Monthly, 0, 0, UtcOffset::UTC).is_err());
        assert!(CalendarWindow::new(CalendarPeriod::Weekly, 8, 0, UtcOffset::UTC).is_err());
        assert!(CalendarWindow::new(CalendarPeriod::Daily, 1, 24, UtcOffset::UTC).is_err());
    }
}
