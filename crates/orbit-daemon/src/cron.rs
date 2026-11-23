//! Pure-Rust 5-field POSIX Cron parser, preset compiler, and timezone-aware next-trigger calculator.
//!
//! Supports standard 5-field syntax (`minute hour dom month dow`):
//! - `*`, `*/N`, `A`, `A-B`, `A-B/N`, comma-separated lists (`1,15,30`)
//! - Friendly presets compiled to canonical 5-field cron:
//!   `1m`, `5m`, `10m`, `15m`, `30m`, `1h`, `2h`, `6h`, `12h`, `hourly`, `daily`, `daily HH:MM`, `weekly`
//! - Pure-Rust timezone scheduling via [`ScheduleTimezone`]:
//!   - Named IANA timezones (`Asia/Shanghai`, `America/New_York`, `UTC`, etc.) via `chrono-tz`
//!   - Canonical fixed UTC offsets (`±HH:MM`, e.g. `+08:00`, `-05:00`) via `chrono::FixedOffset`
//!   - Host system local timezone (`chrono::Local`) when omitted or empty

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, NaiveDate, NaiveDateTime,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;

/// Target timezone for evaluating cron wall-clock fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleTimezone {
    /// Host system local timezone (`chrono::Local`), used when `timezone` is `None` or empty.
    Local,
    /// Named IANA timezone (e.g. `Asia/Shanghai`, `America/New_York`, `UTC`).
    Iana(Tz),
    /// Canonical fixed UTC offset (`±HH:MM`, e.g. `+08:00`, `-05:00`).
    Fixed(FixedOffset),
}

impl ScheduleTimezone {
    /// Parse an optional timezone specification.
    ///
    /// - `None` or empty/whitespace-only string defaults to [`ScheduleTimezone::Local`].
    /// - Named IANA identifiers (e.g. `Asia/Shanghai`, `America/New_York`, `UTC`) resolve via `chrono-tz`.
    /// - Canonical fixed offsets (`±HH:MM`, e.g. `+08:00`, `-05:00`) resolve via `FixedOffset`.
    /// - Any other value is rejected with an explicit error (never silently falling back to UTC).
    pub fn parse(raw: Option<&str>) -> anyhow::Result<Self> {
        let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::Local);
        };

        if let Ok(tz) = trimmed.parse::<Tz>() {
            return Ok(Self::Iana(tz));
        }
        if let Some(offset) = parse_fixed_offset(trimmed) {
            return Ok(Self::Fixed(offset));
        }

        anyhow::bail!(
            "invalid timezone `{trimmed}`: expected an IANA timezone (e.g. `Asia/Shanghai`, `America/New_York`, `UTC`) or fixed UTC offset (`±HH:MM`, e.g. `+08:00`, `-05:00`)"
        );
    }

    /// Format a UTC instant in this timezone using a `chrono` strftime pattern.
    pub fn format_utc(&self, dt: DateTime<Utc>, fmt: &str) -> String {
        match self {
            Self::Local => dt.with_timezone(&Local).format(fmt).to_string(),
            Self::Iana(tz) => dt.with_timezone(tz).format(fmt).to_string(),
            Self::Fixed(offset) => dt.with_timezone(offset).format(fmt).to_string(),
        }
    }
}

/// Validate and normalize an optional timezone string, returning both the normalized stored value
/// (`None` for empty/omitted -> host local timezone) and the parsed [`ScheduleTimezone`].
pub fn parse_and_normalize_timezone(
    raw: Option<&str>,
) -> anyhow::Result<(Option<String>, ScheduleTimezone)> {
    let parsed = ScheduleTimezone::parse(raw)?;
    let normalized = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    Ok((normalized, parsed))
}

/// Parse a canonical fixed UTC offset in `±HH:MM` format (6 ASCII bytes, `00..=23` hours, `00..=59` minutes).
fn parse_fixed_offset(s: &str) -> Option<FixedOffset> {
    let bytes = s.as_bytes();
    if bytes.len() != 6 || bytes[3] != b':' {
        return None;
    }
    let is_east = match bytes[0] {
        b'+' => true,
        b'-' => false,
        _ => return None,
    };
    if !bytes[1].is_ascii_digit()
        || !bytes[2].is_ascii_digit()
        || !bytes[4].is_ascii_digit()
        || !bytes[5].is_ascii_digit()
    {
        return None;
    }
    let hours = i32::from((bytes[1] - b'0') * 10 + (bytes[2] - b'0'));
    let minutes = i32::from((bytes[4] - b'0') * 10 + (bytes[5] - b'0'));
    if hours > 23 || minutes > 59 {
        return None;
    }
    let total_secs = hours * 3600 + minutes * 60;
    if is_east {
        FixedOffset::east_opt(total_secs)
    } else {
        FixedOffset::west_opt(total_secs)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    pub expr: String,
    minutes: [bool; 60],
    hours: [bool; 24],
    doms: [bool; 32],   // 1..=31
    months: [bool; 13], // 1..=12
    dows: [bool; 7],    // 0 (Sun) ..= 6 (Sat)
    dom_any: bool,
    dow_any: bool,
}

/// Normalize a user-provided cron expression or preset shorthand (e.g. `"5m"`, `"1h"`, `"daily 09:30"`)
/// into a canonical 5-field cron expression.
pub fn normalize_cron_expr(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("cron expression cannot be empty");
    }
    let lower = trimmed.to_ascii_lowercase();
    let compiled = match lower.as_str() {
        "@hourly" | "hourly" => "0 * * * *".to_string(),
        "@daily" | "daily" => "0 9 * * *".to_string(),
        "@weekdays" | "weekdays" => "0 9 * * 1-5".to_string(),
        "@weekly" | "weekly" => "0 9 * * 1".to_string(),
        _ if lower.starts_with("daily ") || lower.starts_with("weekdays ") => {
            let (prefix, dow) = if lower.starts_with("weekdays ") {
                ("weekdays ", "1-5")
            } else {
                ("daily ", "*")
            };
            let hm = lower.trim_start_matches(prefix).trim();
            let (h_s, m_s) = hm
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("expected `{}HH:MM`, got `{raw}`", prefix))?;
            let h: u32 = h_s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid hour in `{raw}`"))?;
            let m: u32 = m_s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid minute in `{raw}`"))?;
            if h > 23 || m > 59 {
                anyhow::bail!("hour/minute out of range in `{raw}`");
            }
            format!("{m} {h} * * {dow}")
        }
        _ if lower.ends_with('m')
            && lower[..lower.len() - 1].chars().all(|c| c.is_ascii_digit()) =>
        {
            let mins: u32 = lower[..lower.len() - 1].parse()?;
            if mins == 0 || mins > 59 {
                anyhow::bail!("minute preset must be between 1m and 59m, got `{raw}`");
            }
            if mins == 1 {
                "* * * * *".to_string()
            } else {
                format!("*/{mins} * * * *")
            }
        }
        _ if lower.ends_with('h')
            && lower[..lower.len() - 1].chars().all(|c| c.is_ascii_digit()) =>
        {
            let hrs: u32 = lower[..lower.len() - 1].parse()?;
            if hrs == 0 || hrs > 23 {
                anyhow::bail!("hour preset must be between 1h and 23h, got `{raw}`");
            }
            if hrs == 1 {
                "0 * * * *".to_string()
            } else {
                format!("0 */{hrs} * * *")
            }
        }
        _ => trimmed.to_string(),
    };

    // Validate immediately by parsing
    let _ = CronSchedule::parse(&compiled)?;
    Ok(compiled)
}

impl CronSchedule {
    pub fn parse(expr: &str) -> anyhow::Result<Self> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            anyhow::bail!(
                "cron expression must have 5 fields (minute hour dom month dow), got `{expr}`"
            );
        }
        let mut minutes = [false; 60];
        let mut hours = [false; 24];
        let mut doms = [false; 32];
        let mut months = [false; 13];
        let mut dows = [false; 7];

        parse_field(fields[0], 0, 59, &mut minutes, false)?;
        parse_field(fields[1], 0, 23, &mut hours, false)?;
        parse_field(fields[2], 1, 31, &mut doms, false)?;
        parse_field(fields[3], 1, 12, &mut months, false)?;
        parse_field(fields[4], 0, 7, &mut dows, true)?;

        Ok(Self {
            expr: expr.to_string(),
            minutes,
            hours,
            doms,
            months,
            dows,
            dom_any: fields[2] == "*",
            dow_any: fields[4] == "*",
        })
    }

    fn day_matches(&self, dom: usize, dow: usize) -> bool {
        let dom_ok = self.doms[dom];
        let dow_ok = self.dows[dow];
        if self.dom_any && self.dow_any {
            true
        } else if self.dom_any {
            dow_ok
        } else if self.dow_any {
            dom_ok
        } else {
            // Standard POSIX cron semantics: OR when both dom and dow are restricted
            dom_ok || dow_ok
        }
    }

    pub fn matches(&self, dt: DateTime<Utc>) -> bool {
        let min = dt.minute() as usize;
        let hr = dt.hour() as usize;
        let dom = dt.day() as usize;
        let mon = dt.month() as usize;
        let dow = dt.weekday().num_days_from_sunday() as usize;

        self.minutes[min] && self.hours[hr] && self.months[mon] && self.day_matches(dom, dow)
    }

    /// Check whether a UTC instant represents a canonical firing tick of this schedule in `tz`
    /// (whole minute boundary, matching wall-clock fields, and earliest occurrence on DST fall-back).
    pub fn matches_in_timezone(&self, dt: DateTime<Utc>, tz: ScheduleTimezone) -> bool {
        match tz {
            ScheduleTimezone::Local => self.matches_in_tz_impl(dt, &Local),
            ScheduleTimezone::Iana(iana) => self.matches_in_tz_impl(dt, &iana),
            ScheduleTimezone::Fixed(offset) => self.matches_in_tz_impl(dt, &offset),
        }
    }

    fn matches_in_tz_impl<T: TimeZone>(&self, dt: DateTime<Utc>, tz: &T) -> bool {
        let local = dt.with_timezone(tz);
        if local.second() != 0 || local.nanosecond() != 0 {
            return false;
        }
        let naive = local.naive_local();
        if !self.minutes[naive.minute() as usize]
            || !self.hours[naive.hour() as usize]
            || !self.months[naive.month() as usize]
            || !self.day_matches(
                naive.day() as usize,
                naive.weekday().num_days_from_sunday() as usize,
            )
        {
            return false;
        }
        match tz.from_local_datetime(&naive) {
            LocalResult::Single(resolved) => resolved.with_timezone(&Utc) == dt,
            LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc) == dt,
            LocalResult::None => false,
        }
    }

    /// Compute the next UTC timestamp strictly after `after`, evaluating cron fields in UTC.
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.next_after_in_tz_impl(after, &Utc)
    }

    /// Compute the next canonical UTC timestamp strictly after `after`, evaluating cron fields
    /// in the parsed [`ScheduleTimezone`].
    ///
    /// Handles DST transitions cleanly:
    /// - Spring-forward gaps (nonexistent wall-clock times): skipped without looping.
    /// - Fall-back repeated hours (ambiguous wall-clock times): resolves to the earliest occurrence
    ///   so each wall-clock cron tick fires at most once.
    pub fn next_after_in_timezone(
        &self,
        after: DateTime<Utc>,
        tz: ScheduleTimezone,
    ) -> Option<DateTime<Utc>> {
        match tz {
            ScheduleTimezone::Local => self.next_after_in_tz_impl(after, &Local),
            ScheduleTimezone::Iana(iana) => self.next_after_in_tz_impl(after, &iana),
            ScheduleTimezone::Fixed(offset) => self.next_after_in_tz_impl(after, &offset),
        }
    }

    /// Compute the next canonical UTC timestamp strictly after `after`, evaluating cron fields
    /// in `tz` (defaulting to host system local timezone when `tz` is `None` or empty).
    pub fn next_after_in_tz(
        &self,
        after: DateTime<Utc>,
        tz: Option<&str>,
    ) -> anyhow::Result<Option<DateTime<Utc>>> {
        let resolved = ScheduleTimezone::parse(tz)?;
        Ok(self.next_after_in_timezone(after, resolved))
    }

    fn next_after_in_tz_impl<T: TimeZone>(
        &self,
        after: DateTime<Utc>,
        tz: &T,
    ) -> Option<DateTime<Utc>> {
        let after_local = after.with_timezone(tz);
        // Start at the next whole minute in the target timezone's local wall-clock.
        let mut cand: NaiveDateTime = after_local
            .naive_local()
            .with_second(0)?
            .with_nanosecond(0)?
            + Duration::minutes(1);

        // Search up to 8 years forward (covers Feb 29 schedules across non-leap century boundaries
        // while guaranteeing termination for impossible dates like Feb 31).
        let limit = cand + Duration::days(366 * 4 + 365 * 4);
        while cand < limit {
            if !self.months[cand.month() as usize] {
                let (next_year, next_month) = if cand.month() == 12 {
                    (cand.year() + 1, 1)
                } else {
                    (cand.year(), cand.month() + 1)
                };
                cand = NaiveDate::from_ymd_opt(next_year, next_month, 1)?.and_hms_opt(0, 0, 0)?;
                continue;
            }

            if !self.day_matches(
                cand.day() as usize,
                cand.weekday().num_days_from_sunday() as usize,
            ) {
                cand = cand.date().succ_opt()?.and_hms_opt(0, 0, 0)?;
                continue;
            }

            if !self.hours[cand.hour() as usize] {
                cand = (cand + Duration::hours(1)).with_minute(0)?;
                continue;
            }

            if !self.minutes[cand.minute() as usize] {
                cand += Duration::minutes(1);
                continue;
            }

            match tz.from_local_datetime(&cand) {
                LocalResult::Single(dt) => {
                    let utc = dt.with_timezone(&Utc);
                    if utc > after {
                        return Some(utc);
                    }
                }
                LocalResult::Ambiguous(earliest, _latest) => {
                    // Fall-back repeated hour: fire only on the earliest wall-clock occurrence
                    // so the same cron tick never double-fires.
                    let utc = earliest.with_timezone(&Utc);
                    if utc > after {
                        return Some(utc);
                    }
                }
                LocalResult::None => {
                    // Spring-forward gap: wall-clock time does not exist in this timezone; skip.
                }
            }
            cand += Duration::minutes(1);
        }
        None
    }
}

fn parse_field(
    field: &str,
    min: usize,
    max: usize,
    out: &mut [bool],
    wrap_seven_to_zero: bool,
) -> anyhow::Result<()> {
    for part in field.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            anyhow::bail!("empty cron list element in `{field}`");
        }
        let (range_part, step) = if let Some((lhs, rhs)) = trimmed.split_once('/') {
            let s: usize = rhs
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid cron step `{rhs}` in `{field}`"))?;
            if s == 0 {
                anyhow::bail!("cron step cannot be 0 in `{field}`");
            }
            (lhs, s)
        } else {
            (trimmed, 1)
        };

        let (start, end) = if range_part == "*" {
            (min, if wrap_seven_to_zero { 6 } else { max })
        } else if let Some((a_s, b_s)) = range_part.split_once('-') {
            let a: usize = a_s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid cron range start `{a_s}`"))?;
            let b: usize = b_s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid cron range end `{b_s}`"))?;
            if a < min || b > max || a > b {
                anyhow::bail!("cron range `{a}-{b}` out of bounds ({min}..={max})");
            }
            (a, b)
        } else {
            let v: usize = range_part
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid cron value `{range_part}`"))?;
            if v < min || v > max {
                anyhow::bail!("cron value `{v}` out of bounds ({min}..={max})");
            }
            (v, v)
        };

        let mut cur = start;
        while cur <= end {
            let idx = if wrap_seven_to_zero && cur == 7 {
                0
            } else {
                cur
            };
            if idx < out.len() {
                out[idx] = true;
            }
            cur += step;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s)
            .single()
            .expect("valid utc timestamp")
    }

    #[test]
    fn normalizes_presets_and_computes_next_fire() {
        assert_eq!(normalize_cron_expr("5m").expect("5m"), "*/5 * * * *");
        assert_eq!(normalize_cron_expr("1m").expect("1m"), "* * * * *");
        assert_eq!(normalize_cron_expr("2h").expect("2h"), "0 */2 * * *");
        assert_eq!(
            normalize_cron_expr("daily 09:30").expect("daily"),
            "30 9 * * *"
        );
        assert_eq!(
            normalize_cron_expr("weekdays").expect("weekdays"),
            "0 9 * * 1-5"
        );
        assert_eq!(
            normalize_cron_expr("weekdays 08:45").expect("weekdays time"),
            "45 8 * * 1-5"
        );

        let sched = CronSchedule::parse("*/15 * * * *").expect("parse");
        let base = utc(2026, 11, 20, 10, 14, 30);
        let next = sched.next_after(base).expect("next");
        assert_eq!(next, utc(2026, 11, 20, 10, 15, 0));

        let next2 = sched.next_after(next).expect("next2");
        assert_eq!(next2, utc(2026, 11, 20, 10, 30, 0));
    }

    #[test]
    fn rejects_invalid_cron_expressions() {
        assert!(normalize_cron_expr("").is_err());
        assert!(normalize_cron_expr("0m").is_err());
        assert!(normalize_cron_expr("*/0 * * * *").is_err());
        assert!(normalize_cron_expr("60 * * * *").is_err());
        assert!(normalize_cron_expr("* * *").is_err());
    }

    #[test]
    fn schedules_in_asia_shanghai_timezone() {
        // 09:00 Asia/Shanghai == 01:00 UTC (UTC+8, no DST)
        let sched = CronSchedule::parse("0 9 * * *").expect("parse");

        // 00:30 UTC == 08:30 Asia/Shanghai -> fires same day at 09:00 CST == 01:00 UTC
        let before = utc(2026, 6, 15, 0, 30, 0);
        let next = sched
            .next_after_in_tz(before, Some("Asia/Shanghai"))
            .expect("valid tz")
            .expect("next fire");
        assert_eq!(next, utc(2026, 6, 15, 1, 0, 0));

        // After firing at 01:00 UTC (09:00 CST), next fire is next day 01:00 UTC
        let next2 = sched
            .next_after_in_tz(next, Some("Asia/Shanghai"))
            .expect("valid tz")
            .expect("next fire 2");
        assert_eq!(next2, utc(2026, 6, 16, 1, 0, 0));
    }

    #[test]
    fn handles_america_new_york_dst_spring_forward() {
        // 2026-03-08: America/New_York springs forward at 02:00 EST (07:00Z) -> 03:00 EDT (07:00Z).
        // Wall-clock times 02:00..=02:59 do NOT exist on 2026-03-08.

        // 1. Daily at 02:30: nonexistent on 2026-03-08 -> skips cleanly to 2026-03-09 02:30 EDT (06:30Z)
        let sched_gap = CronSchedule::parse("30 2 * * *").expect("parse");
        let before_gap = utc(2026, 3, 8, 6, 0, 0); // 2026-03-08 01:00 EST
        let next_gap = sched_gap
            .next_after_in_tz(before_gap, Some("America/New_York"))
            .expect("valid tz")
            .expect("next fire after gap");
        assert_eq!(next_gap, utc(2026, 3, 9, 6, 30, 0));

        // 2. Hourly at :00: 01:00 EST (06:00Z) -> skips 02:00 gap -> 03:00 EDT (07:00Z)
        let sched_hourly = CronSchedule::parse("0 * * * *").expect("parse");
        let next_hour = sched_hourly
            .next_after_in_tz(before_gap, Some("America/New_York"))
            .expect("valid tz")
            .expect("next hour");
        assert_eq!(next_hour, utc(2026, 3, 8, 7, 0, 0));

        // 3. Daily at 09:00 across spring-forward:
        //    2026-03-07 09:00 EST (14:00Z) -> 2026-03-08 09:00 EDT (13:00Z, 23 real hours later)
        let sched_daily = CronSchedule::parse("0 9 * * *").expect("parse");
        let prev_day = utc(2026, 3, 7, 14, 0, 0);
        let next_day = sched_daily
            .next_after_in_tz(prev_day, Some("America/New_York"))
            .expect("valid tz")
            .expect("next daily");
        assert_eq!(next_day, utc(2026, 3, 8, 13, 0, 0));
    }

    #[test]
    fn handles_america_new_york_dst_fall_back_without_double_firing() {
        // 2026-11-01: America/New_York falls back at 02:00 EDT (06:00Z) -> 01:00 EST (06:00Z).
        // Wall-clock 01:30 occurs twice: 05:30Z (EDT, earliest) and 06:30Z (EST, latest).
        let sched = CronSchedule::parse("30 1 * * *").expect("parse");
        let ny = ScheduleTimezone::Iana(Tz::America__New_York);

        // 1. Before fall-back (00:30 EDT == 04:30Z): fires on FIRST occurrence (01:30 EDT == 05:30Z)
        let before = utc(2026, 11, 1, 4, 30, 0);
        let first = sched
            .next_after_in_tz(before, Some("America/New_York"))
            .expect("valid tz")
            .expect("first fire");
        assert_eq!(first, utc(2026, 11, 1, 5, 30, 0));
        assert!(sched.matches_in_timezone(first, ny));
        assert!(!sched.matches_in_timezone(utc(2026, 11, 1, 6, 30, 0), ny));

        // 2. Immediately after firing at 01:30 EDT (05:30Z): MUST NOT fire again at 01:30 EST (06:30Z);
        //    next fire must be 2026-11-02 01:30 EST (06:30Z).
        let after_first = sched
            .next_after_in_tz(first, Some("America/New_York"))
            .expect("valid tz")
            .expect("second day fire");
        assert_eq!(after_first, utc(2026, 11, 2, 6, 30, 0));

        // 3. Even if queried during the repeated hour at 01:15 EST (06:15Z), the 01:30 tick
        //    already occurred at 05:30Z so it must NOT fire at 06:30Z; advances to 2026-11-02.
        let during_repeat = utc(2026, 11, 1, 6, 15, 0);
        let from_repeat = sched
            .next_after_in_tz(during_repeat, Some("America/New_York"))
            .expect("valid tz")
            .expect("after repeat");
        assert_eq!(from_repeat, utc(2026, 11, 2, 6, 30, 0));

        // 4. Daily at 09:00 across fall-back:
        //    2026-10-31 09:00 EDT (13:00Z) -> 2026-11-01 09:00 EST (14:00Z, 25 real hours later)
        let sched_daily = CronSchedule::parse("0 9 * * *").expect("parse");
        let prev_day = utc(2026, 10, 31, 13, 0, 0);
        let next_day = sched_daily
            .next_after_in_tz(prev_day, Some("America/New_York"))
            .expect("valid tz")
            .expect("next daily");
        assert_eq!(next_day, utc(2026, 11, 1, 14, 0, 0));
    }

    #[test]
    fn supports_canonical_fixed_offsets_and_utc() {
        let sched = CronSchedule::parse("0 9 * * *").expect("parse");
        // +09:00: 09:00+09:00 == 00:00 UTC
        let base = utc(2026, 5, 10, 23, 30, 0); // 2026-05-11 08:30+09:00
        let next = sched
            .next_after_in_tz(base, Some("+09:00"))
            .expect("valid +09:00")
            .expect("next fire");
        assert_eq!(next, utc(2026, 5, 11, 0, 0, 0));

        assert_eq!(
            ScheduleTimezone::parse(Some("-05:30")).expect("-05:30"),
            ScheduleTimezone::Fixed(FixedOffset::west_opt(5 * 3600 + 30 * 60).expect("-5h30m"))
        );
        assert_eq!(
            ScheduleTimezone::parse(Some("+00:00")).expect("+00:00"),
            ScheduleTimezone::Fixed(FixedOffset::east_opt(0).expect("+00:00"))
        );
        assert_eq!(
            ScheduleTimezone::parse(Some("UTC")).expect("UTC"),
            ScheduleTimezone::Iana(Tz::UTC)
        );
    }

    #[test]
    fn defaults_none_and_empty_to_host_local_timezone() {
        assert_eq!(
            ScheduleTimezone::parse(None).expect("None"),
            ScheduleTimezone::Local
        );
        assert_eq!(
            ScheduleTimezone::parse(Some("")).expect("empty"),
            ScheduleTimezone::Local
        );
        assert_eq!(
            ScheduleTimezone::parse(Some("   ")).expect("whitespace"),
            ScheduleTimezone::Local
        );
        assert_eq!(
            parse_and_normalize_timezone(Some("  ")).expect("norm empty"),
            (None, ScheduleTimezone::Local)
        );

        let sched = CronSchedule::parse("30 9 * * *").expect("parse");
        let base = utc(2026, 7, 1, 0, 0, 0);
        let next = sched
            .next_after_in_tz(base, None)
            .expect("local tz")
            .expect("next local fire");
        assert!(next > base);
        let next_local = next.with_timezone(&Local);
        assert_eq!(next_local.hour(), 9);
        assert_eq!(next_local.minute(), 30);
        assert_eq!(next_local.second(), 0);
    }

    #[test]
    fn rejects_invalid_and_non_ascii_timezones_without_panicking() {
        for bad in [
            "éé",
            "北京",
            "Asia/上海",
            "Invalid/Zone",
            "Mars/Olympus",
            "+25:00",
            "-24:00",
            "+08:60",
            "UTC+8",
            "GMT+9",
            "+0800",
            "+8",
            "Z",
            "utc",
            "+8:05",
            "+08:5",
            "NotATimezone",
            "+08:00:00",
        ] {
            let err =
                ScheduleTimezone::parse(Some(bad)).expect_err(&format!("must reject `{bad}`"));
            assert!(
                err.to_string().contains("invalid timezone"),
                "unexpected error for `{bad}`: {err}"
            );
            assert!(parse_and_normalize_timezone(Some(bad)).is_err());
            let sched = CronSchedule::parse("0 9 * * *").expect("parse");
            assert!(
                sched
                    .next_after_in_tz(utc(2026, 1, 1, 0, 0, 0), Some(bad))
                    .is_err()
            );
        }
    }
}
