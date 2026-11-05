use time::{
    format_description::well_known::Rfc3339, Date, OffsetDateTime, PrimitiveDateTime, Time,
    UtcOffset,
};

const MAX_INTERVAL_SECONDS: u64 = 366 * 24 * 60 * 60;
const MAX_CRON_SEARCH_DAYS: usize = 10 * 366;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    Once(OffsetDateTime),
    Every { seconds: u64 },
    Cron(CronSchedule),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    minute: Field,
    hour: Field,
    day_of_month: Field,
    month: Field,
    day_of_week: Field,
    offset: UtcOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Field {
    mask: u64,
    wildcard: bool,
}

impl Field {
    fn contains(self, value: u8) -> bool {
        self.mask & (1u64 << value) != 0
    }

    fn parse(raw: &str, min: u8, max: u8, weekday: bool) -> Result<Self, String> {
        let mut mask = 0u64;
        if raw.is_empty() {
            return Err("empty cron field".into());
        }
        for item in raw.split(',') {
            let (range, step) = match item.split_once('/') {
                Some((range, step)) => {
                    let step = step
                        .parse::<u8>()
                        .map_err(|_| format!("invalid step in {item:?}"))?;
                    if step == 0 {
                        return Err(format!("zero step in {item:?}"));
                    }
                    (range, step)
                }
                None => (item, 1),
            };
            let (start, end) = if range == "*" {
                (min, max)
            } else if let Some((start, end)) = range.split_once('-') {
                (parse_number(start, min, max)?, parse_number(end, min, max)?)
            } else {
                let start = parse_number(range, min, max)?;
                (start, if item.contains('/') { max } else { start })
            };
            if start > end {
                return Err(format!("reversed cron range {range:?}"));
            }
            for value in (start..=end).step_by(usize::from(step)) {
                let value = if weekday && value == 7 { 0 } else { value };
                mask |= 1u64 << value;
            }
        }
        if mask == 0 {
            return Err(format!("empty cron field {raw:?}"));
        }
        let full = (min..=max).fold(0u64, |bits, value| {
            let value = if weekday && value == 7 { 0 } else { value };
            bits | (1u64 << value)
        });
        Ok(Self {
            mask,
            wildcard: mask == full,
        })
    }
}

fn parse_number(raw: &str, min: u8, max: u8) -> Result<u8, String> {
    let value = raw
        .parse::<u8>()
        .map_err(|_| format!("invalid cron number {raw:?}"))?;
    if !(min..=max).contains(&value) {
        return Err(format!("cron number {value} outside {min}..={max}"));
    }
    Ok(value)
}

fn duration_seconds(raw: &str) -> Result<u64, String> {
    let split = raw
        .find(|ch: char| !ch.is_ascii_digit())
        .ok_or_else(|| "duration needs a unit (s, m, h, d, w)".to_owned())?;
    let amount = raw[..split]
        .parse::<u64>()
        .map_err(|_| "invalid duration amount".to_owned())?;
    let multiplier = match &raw[split..] {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return Err("duration unit must be s, m, h, d or w".into()),
    };
    let seconds = amount.checked_mul(multiplier).ok_or("duration overflow")?;
    if seconds == 0 || seconds > MAX_INTERVAL_SECONDS {
        return Err("duration must be between 1 second and 366 days".into());
    }
    Ok(seconds)
}

impl Schedule {
    /// Parses the user-facing expression. Bare durations are one-shot relative
    /// to `now`; `every <duration>` is an interval. Five-field cron uses the
    /// supplied fixed UTC offset. RFC3339 timestamps are absolute one-shots.
    pub fn parse(raw: &str, now: OffsetDateTime, offset: UtcOffset) -> Result<Self, String> {
        let raw = raw.trim();
        if let Some(duration) = raw.strip_prefix("every ") {
            return Ok(Self::Every {
                seconds: duration_seconds(duration.trim())?,
            });
        }
        if let Ok(seconds) = duration_seconds(raw) {
            let seconds = i64::try_from(seconds).map_err(|_| "duration overflow")?;
            return now
                .checked_add(time::Duration::seconds(seconds))
                .map(Self::Once)
                .ok_or_else(|| "one-shot timestamp overflow".into());
        }
        if let Ok(at) = OffsetDateTime::parse(raw, &Rfc3339) {
            if at <= now {
                return Err("one-shot timestamp must be in the future".into());
            }
            return Ok(Self::Once(at));
        }
        Ok(Self::Cron(CronSchedule::parse(raw, offset)?))
    }

    /// Returns the next occurrence strictly after `previous`. For intervals,
    /// pass the *scheduled* occurrence rather than completion time to avoid
    /// drift. Returns None after a one-shot has run or on date overflow.
    pub fn next_after(&self, previous: OffsetDateTime) -> Option<OffsetDateTime> {
        match self {
            Self::Once(at) => (previous < *at).then_some(*at),
            Self::Every { seconds } => {
                previous.checked_add(time::Duration::seconds(*seconds as i64))
            }
            Self::Cron(cron) => cron.next_after(previous),
        }
    }
}

impl CronSchedule {
    pub fn parse(raw: &str, offset: UtcOffset) -> Result<Self, String> {
        let parts: Vec<&str> = raw.split_whitespace().collect();
        if parts.len() != 5 {
            return Err("cron expression must have five fields".into());
        }
        Ok(Self {
            minute: Field::parse(parts[0], 0, 59, false)?,
            hour: Field::parse(parts[1], 0, 23, false)?,
            day_of_month: Field::parse(parts[2], 1, 31, false)?,
            month: Field::parse(parts[3], 1, 12, false)?,
            day_of_week: Field::parse(parts[4], 0, 7, true)?,
            offset,
        })
    }

    fn day_matches(&self, local: Date) -> bool {
        let dom = self.day_of_month.contains(local.day());
        let dow = self
            .day_of_week
            .contains(local.weekday().number_days_from_sunday());
        let day_match = match (self.day_of_month.wildcard, self.day_of_week.wildcard) {
            (false, false) => dom || dow,
            (false, true) => dom,
            (true, false) => dow,
            (true, true) => true,
        };
        self.month.contains(local.month() as u8) && day_match
    }

    pub fn next_after(&self, previous: OffsetDateTime) -> Option<OffsetDateTime> {
        let start = previous
            .unix_timestamp()
            .div_euclid(60)
            .checked_add(1)?
            .checked_mul(60)?;
        let mut date = OffsetDateTime::from_unix_timestamp(start)
            .ok()?
            .to_offset(self.offset)
            .date();
        for _ in 0..MAX_CRON_SEARCH_DAYS {
            if self.day_matches(date) {
                let midnight =
                    PrimitiveDateTime::new(date, Time::MIDNIGHT).assume_offset(self.offset);
                for hour in 0..24 {
                    if !self.hour.contains(hour) {
                        continue;
                    }
                    for minute in 0..60 {
                        if !self.minute.contains(minute) {
                            continue;
                        }
                        let at = midnight
                            .checked_add(time::Duration::hours(i64::from(hour)))?
                            .checked_add(time::Duration::minutes(i64::from(minute)))?;
                        if at.unix_timestamp() >= start {
                            return Some(at);
                        }
                    }
                }
            }
            date = date.next_day()?;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).unwrap()
    }

    #[test]
    fn relative_one_shot_interval_and_rfc3339_have_distinct_semantics() {
        let now = at("2026-10-06T08:00:00Z");
        let once = Schedule::parse("30m", now, UtcOffset::UTC).unwrap();
        assert_eq!(once.next_after(now), Some(at("2026-10-06T08:30:00Z")));
        assert_eq!(once.next_after(at("2026-10-06T08:30:00Z")), None);
        let every = Schedule::parse("every 2h", now, UtcOffset::UTC).unwrap();
        assert_eq!(every.next_after(now), Some(at("2026-10-06T10:00:00Z")));
        assert_eq!(
            every.next_after(at("2026-10-06T10:00:00Z")),
            Some(at("2026-10-06T12:00:00Z"))
        );
        assert_eq!(
            Schedule::parse("2026-10-06T09:00:00Z", now, UtcOffset::UTC)
                .unwrap()
                .next_after(now),
            Some(at("2026-10-06T09:00:00Z"))
        );
    }

    #[test]
    fn cron_uses_fixed_offset_and_advances_from_scheduled_minute() {
        let offset = UtcOffset::from_hms(-3, 0, 0).unwrap();
        let schedule = Schedule::parse("0 9 * * 1-5", at("2026-10-06T11:59:33Z"), offset).unwrap();
        assert_eq!(
            schedule.next_after(at("2026-10-06T11:59:33Z")),
            Some(at("2026-10-06T12:00:00Z"))
        );
        assert_eq!(
            schedule.next_after(at("2026-10-06T12:00:00Z")),
            Some(at("2026-10-07T12:00:00Z"))
        );
    }

    #[test]
    fn cron_dom_and_weekday_follow_standard_or_rule() {
        let cron = CronSchedule::parse("0 9 1 * 0", UtcOffset::UTC).unwrap();
        // Thu 1 Oct matches day-of-month; Sun 4 Oct matches weekday.
        assert_eq!(
            cron.next_after(at("2026-09-30T10:00:00Z")),
            Some(at("2026-10-01T09:00:00Z"))
        );
        assert_eq!(
            cron.next_after(at("2026-10-01T09:00:00Z")),
            Some(at("2026-10-04T09:00:00Z"))
        );
    }

    #[test]
    fn rejects_invalid_fields_and_intervals() {
        let now = at("2026-10-06T08:00:00Z");
        for expr in [
            "* * * *",
            "*/0 * * * *",
            "60 * * * *",
            "0 24 * * *",
            "0 0 0 * *",
            "0 0 * 13 *",
            "0 0 * * 8",
            "5-3 * * * *",
            "every 0s",
            "every 999999d",
            "2026-10-06T07:00:00Z",
        ] {
            assert!(
                Schedule::parse(expr, now, UtcOffset::UTC).is_err(),
                "{expr}"
            );
        }
    }

    #[test]
    fn impossible_calendar_day_returns_none_without_minute_scan() {
        let now = at("2026-10-06T08:00:00Z");
        let cron = CronSchedule::parse("0 9 31 2 *", UtcOffset::UTC).unwrap();
        assert_eq!(cron.next_after(now), None);
    }
}
