//! Pure-Rust zero-dependency 5-field POSIX Cron parser, preset compiler, and next-trigger calculator.
//!
//! Supports standard 5-field syntax (`minute hour dom month dow`):
//! - `*`, `*/N`, `A`, `A-B`, `A-B/N`, comma-separated lists (`1,15,30`)
//! - Friendly presets compiled to canonical 5-field cron:
//!   `1m`, `5m`, `10m`, `15m`, `30m`, `1h`, `2h`, `6h`, `12h`, `hourly`, `daily`, `daily HH:MM`, `weekly`

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

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
        "@weekly" | "weekly" => "0 9 * * 1".to_string(),
        _ if lower.starts_with("daily ") => {
            let hm = lower.trim_start_matches("daily ").trim();
            let (h_s, m_s) = hm
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("expected `daily HH:MM`, got `{raw}`"))?;
            let h: u32 = h_s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid hour in `{raw}`"))?;
            let m: u32 = m_s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid minute in `{raw}`"))?;
            if h > 23 || m > 59 {
                anyhow::bail!("hour/minute out of range in `{raw}`");
            }
            format!("{m} {h} * * *")
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

    pub fn matches(&self, dt: DateTime<Utc>) -> bool {
        let min = dt.minute() as usize;
        let hr = dt.hour() as usize;
        let dom = dt.day() as usize;
        let mon = dt.month() as usize;
        let dow = dt.weekday().num_days_from_sunday() as usize;

        if !self.minutes[min] || !self.hours[hr] || !self.months[mon] {
            return false;
        }
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

    /// Compute the next UTC timestamp strictly after `after` (rounded up to the next minute boundary).
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Start at the next whole minute
        let mut cand =
            after.with_second(0).and_then(|d| d.with_nanosecond(0))? + Duration::minutes(1);

        // Search up to 4 years forward (handles Feb 29 schedules)
        let limit = cand + Duration::days(366 * 4);
        while cand < limit {
            if !self.months[cand.month() as usize] {
                let (next_year, next_month) = if cand.month() == 12 {
                    (cand.year() + 1, 1)
                } else {
                    (cand.year(), cand.month() + 1)
                };
                cand = chrono::TimeZone::with_ymd_and_hms(&Utc, next_year, next_month, 1, 0, 0, 0)
                    .single()?;
                continue;
            }
            let dom_ok = self.doms[cand.day() as usize];
            let dow_ok = self.dows[cand.weekday().num_days_from_sunday() as usize];
            let day_match = if self.dom_any && self.dow_any {
                true
            } else if self.dom_any {
                dow_ok
            } else if self.dow_any {
                dom_ok
            } else {
                dom_ok || dow_ok
            };
            if !day_match {
                cand = (cand + Duration::days(1)).with_hour(0)?.with_minute(0)?;
                continue;
            }
            if !self.hours[cand.hour() as usize] {
                cand = (cand + Duration::hours(1)).with_minute(0)?;
                continue;
            }
            if self.minutes[cand.minute() as usize] {
                return Some(cand);
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
    use chrono::TimeZone;

    #[test]
    fn normalizes_presets_and_computes_next_fire() {
        assert_eq!(normalize_cron_expr("5m").expect("5m"), "*/5 * * * *");
        assert_eq!(normalize_cron_expr("1m").expect("1m"), "* * * * *");
        assert_eq!(normalize_cron_expr("2h").expect("2h"), "0 */2 * * *");
        assert_eq!(
            normalize_cron_expr("daily 09:30").expect("daily"),
            "30 9 * * *"
        );

        let sched = CronSchedule::parse("*/15 * * * *").expect("parse");
        let base = Utc
            .with_ymd_and_hms(2026, 11, 20, 10, 14, 30)
            .single()
            .expect("base");
        let next = sched.next_after(base).expect("next");
        assert_eq!(
            next,
            Utc.with_ymd_and_hms(2026, 11, 20, 10, 15, 0)
                .single()
                .expect("expected next")
        );

        let next2 = sched.next_after(next).expect("next2");
        assert_eq!(
            next2,
            Utc.with_ymd_and_hms(2026, 11, 20, 10, 30, 0)
                .single()
                .expect("expected next2")
        );
    }

    #[test]
    fn rejects_invalid_cron_expressions() {
        assert!(normalize_cron_expr("").is_err());
        assert!(normalize_cron_expr("0m").is_err());
        assert!(normalize_cron_expr("*/0 * * * *").is_err());
        assert!(normalize_cron_expr("60 * * * *").is_err());
        assert!(normalize_cron_expr("* * *").is_err());
    }
}
