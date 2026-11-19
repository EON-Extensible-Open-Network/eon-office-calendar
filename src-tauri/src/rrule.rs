// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Repeating events (RFC 5545 RRULE), expanded in the event's own wall-clock
//! time, so "every Monday at 09:00, Istanbul" is 09:00 in Istanbul every week
//! whatever daylight saving does elsewhere.
//!
//! Supported: `FREQ` (DAILY, WEEKLY, MONTHLY, YEARLY), `INTERVAL`, `COUNT`,
//! `UNTIL`, `BYDAY` (weekly: days; monthly: days with an optional ordinal such
//! as `2TU` or `-1FR`), `BYMONTHDAY`, `WKST` (accepted, Monday assumed).
//! Anything else is refused with its name -- an imported event keeps its first
//! occurrence and says why it does not repeat, rather than repeating wrongly.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use jiff::civil::{Date, DateTime, Weekday};

const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: i64,
    pub count: Option<u32>,
    /// Inclusive, in the event's wall-clock time.
    pub until: Option<DateTime>,
    /// (ordinal, weekday 0 = Monday). Ordinal 0 = every such day.
    pub by_day: Vec<(i8, u8)>,
    pub by_month_day: Vec<i8>,
}

fn weekday_index(w: Weekday) -> u8 {
    w.to_monday_zero_offset() as u8
}

/// `20261119T090000`, `20261119T090000Z` or `20261119` (end of that day).
pub fn parse_ical_datetime(text: &str) -> Option<DateTime> {
    let t = text.trim_end_matches('Z');
    let (date, time) = t.split_once('T').unwrap_or((t, "235959"));
    if date.len() != 8 || time.len() < 4 {
        return None;
    }
    let num = |s: &str| s.parse::<i32>().ok();
    let d = Date::new(
        num(&date[..4])? as i16,
        num(&date[4..6])? as i8,
        num(&date[6..8])? as i8,
    )
    .ok()?;
    let sec = if time.len() >= 6 {
        num(&time[4..6])?
    } else {
        0
    };
    d.at(
        num(&time[..2])? as i8,
        num(&time[2..4])? as i8,
        sec as i8,
        0,
    )
    .into()
}

impl Rule {
    pub fn parse(text: &str) -> Result<Rule, String> {
        let mut freq = None;
        let mut rule = Rule {
            freq: Freq::Daily,
            interval: 1,
            count: None,
            until: None,
            by_day: Vec::new(),
            by_month_day: Vec::new(),
        };
        for part in text
            .trim()
            .trim_start_matches("RRULE:")
            .split(';')
            .filter(|p| !p.is_empty())
        {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| format!("`{part}` is not KEY=VALUE"))?;
            match key.to_ascii_uppercase().as_str() {
                "FREQ" => {
                    freq = Some(match value {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        other => return Err(format!("FREQ={other} is not supported")),
                    });
                }
                "INTERVAL" => {
                    rule.interval = value
                        .parse()
                        .ok()
                        .filter(|n| (1..=1000).contains(n))
                        .ok_or("INTERVAL is 1-1000")?;
                }
                "COUNT" => {
                    rule.count = Some(
                        value
                            .parse()
                            .ok()
                            .filter(|n| (1..=10_000).contains(n))
                            .ok_or("COUNT is 1-10000")?,
                    );
                }
                "UNTIL" => {
                    rule.until = Some(parse_ical_datetime(value).ok_or("UNTIL is not a date")?);
                }
                "WKST" => {}
                "BYDAY" => {
                    for item in value.split(',') {
                        let day = &item[item.len().saturating_sub(2)..];
                        let ordinal = &item[..item.len().saturating_sub(2)];
                        let at = DAYS
                            .iter()
                            .position(|d| *d == day)
                            .ok_or_else(|| format!("BYDAY={item} is not supported"))?;
                        let n: i8 = if ordinal.is_empty() {
                            0
                        } else {
                            ordinal
                                .trim_start_matches('+')
                                .parse()
                                .ok()
                                .filter(|n: &i8| (-5..=5).contains(n) && *n != 0)
                                .ok_or_else(|| format!("BYDAY={item} is not supported"))?
                        };
                        rule.by_day.push((n, at as u8));
                    }
                }
                "BYMONTHDAY" => {
                    for item in value.split(',') {
                        rule.by_month_day.push(
                            item.parse()
                                .ok()
                                .filter(|n: &i8| (1..=31).contains(n) || (-31..=-1).contains(n))
                                .ok_or("BYMONTHDAY is 1-31 or -31..-1")?,
                        );
                    }
                }
                other => return Err(format!("{other} is not supported")),
            }
        }
        rule.freq = freq.ok_or("FREQ is required")?;
        if rule.count.is_some() && rule.until.is_some() {
            return Err("COUNT and UNTIL together are not allowed (RFC 5545)".into());
        }
        if rule.by_day.iter().any(|(n, _)| *n != 0) && rule.freq != Freq::Monthly {
            return Err("an ordinal BYDAY (like 2TU) is supported with FREQ=MONTHLY only".into());
        }
        if !rule.by_month_day.is_empty() && rule.freq != Freq::Monthly {
            return Err("BYMONTHDAY is supported with FREQ=MONTHLY only".into());
        }
        if !rule.by_day.is_empty() && !matches!(rule.freq, Freq::Weekly | Freq::Monthly) {
            return Err("BYDAY is supported with FREQ=WEEKLY or MONTHLY".into());
        }
        Ok(rule)
    }

    /// Dates matching a monthly rule within one month, in order.
    fn month_days(&self, year: i16, month: i8, start: Date) -> Vec<Date> {
        let Ok(first) = Date::new(year, month, 1) else {
            return Vec::new();
        };
        let len = first.days_in_month();
        let mut out: Vec<Date> = Vec::new();
        if !self.by_day.is_empty() {
            for &(n, wd) in &self.by_day {
                let matching: Vec<Date> = (1..=len)
                    .filter_map(|d| Date::new(year, month, d).ok())
                    .filter(|d| weekday_index(d.weekday()) == wd)
                    .collect();
                match n {
                    0 => out.extend(matching),
                    n if n > 0 => out.extend(matching.get((n - 1) as usize).copied()),
                    n => out.extend(
                        matching
                            .len()
                            .checked_sub((-n) as usize)
                            .and_then(|i| matching.get(i))
                            .copied(),
                    ),
                }
            }
        } else if !self.by_month_day.is_empty() {
            for &n in &self.by_month_day {
                let day = if n > 0 { n } else { len + n + 1 };
                if (1..=len).contains(&day) {
                    out.extend(Date::new(year, month, day).ok());
                }
            }
        } else if start.day() <= len {
            out.extend(Date::new(year, month, start.day()).ok());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Occurrence start times from `start`, in order, until `to` (exclusive),
    /// at most `limit`. The first occurrence is `start` itself.
    pub fn expand(&self, start: DateTime, to: DateTime, limit: usize) -> Vec<DateTime> {
        let time = start.time();
        let mut out: Vec<DateTime> = Vec::new();
        let mut produced: u32 = 0;
        let keep = |dt: DateTime, out: &mut Vec<DateTime>, produced: &mut u32| -> bool {
            if dt < start {
                return true;
            }
            if self.until.is_some_and(|u| dt > u)
                || self.count.is_some_and(|c| *produced >= c)
                || dt >= to
                || out.len() >= limit
            {
                return false;
            }
            *produced += 1;
            out.push(dt);
            true
        };
        let d0 = start.date();
        match self.freq {
            Freq::Daily => {
                let mut k = 0i64;
                while let Ok(d) = d0.checked_add(jiff::Span::new().days(k * self.interval)) {
                    if !keep(d.to_datetime(time), &mut out, &mut produced) {
                        break;
                    }
                    k += 1;
                }
            }
            Freq::Weekly => {
                let days: Vec<u8> = if self.by_day.is_empty() {
                    vec![weekday_index(d0.weekday())]
                } else {
                    self.by_day.iter().map(|(_, d)| *d).collect()
                };
                let monday = d0
                    .checked_sub(jiff::Span::new().days(i64::from(weekday_index(d0.weekday()))))
                    .unwrap_or(d0);
                let mut week = 0i64;
                'weeks: loop {
                    let Ok(base) =
                        monday.checked_add(jiff::Span::new().days(week * 7 * self.interval))
                    else {
                        break;
                    };
                    let mut in_week: Vec<u8> = days.clone();
                    in_week.sort_unstable();
                    in_week.dedup();
                    for wd in in_week {
                        let Ok(d) = base.checked_add(jiff::Span::new().days(i64::from(wd))) else {
                            break 'weeks;
                        };
                        if !keep(d.to_datetime(time), &mut out, &mut produced) {
                            break 'weeks;
                        }
                    }
                    week += 1;
                }
            }
            Freq::Monthly => {
                let mut k = 0i64;
                'months: loop {
                    let months = i64::from(d0.month()) - 1 + k * self.interval;
                    let year = i64::from(d0.year()) + months.div_euclid(12);
                    if year > 9999 {
                        break;
                    }
                    for d in self.month_days(year as i16, (months.rem_euclid(12) + 1) as i8, d0) {
                        if !keep(d.to_datetime(time), &mut out, &mut produced) {
                            break 'months;
                        }
                    }
                    k += 1;
                }
            }
            Freq::Yearly => {
                let mut k = 0i64;
                loop {
                    let year = i64::from(d0.year()) + k * self.interval;
                    if year > 9999 {
                        break;
                    }
                    // 29 February repeats only in leap years.
                    if let Ok(d) = Date::new(year as i16, d0.month(), d0.day()) {
                        if !keep(d.to_datetime(time), &mut out, &mut produced) {
                            break;
                        }
                    }
                    k += 1;
                }
            }
        }
        out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn dt(text: &str) -> DateTime {
        text.parse().unwrap()
    }

    fn dates(rule: &str, start: &str, to: &str) -> Vec<String> {
        Rule::parse(rule)
            .unwrap()
            .expand(dt(start), dt(to), 100)
            .iter()
            .map(|d| d.date().to_string())
            .collect()
    }

    #[test]
    fn rfc_5545_examples() {
        // "Daily for 10 occurrences"
        assert_eq!(
            dates(
                "FREQ=DAILY;COUNT=10",
                "1997-09-02T09:00",
                "1998-01-01T00:00"
            )
            .len(),
            10
        );
        // "Every other week on Monday, Wednesday, and Friday until December 24, 1997"
        let d = dates(
            "FREQ=WEEKLY;INTERVAL=2;WKST=SU;UNTIL=19971224T000000Z;BYDAY=MO,WE,FR",
            "1997-09-01T09:00",
            "1998-12-31T00:00",
        );
        assert_eq!(d.first().map(String::as_str), Some("1997-09-01"));
        assert_eq!(d.last().map(String::as_str), Some("1997-12-22"));
        assert_eq!(d.len(), 25);
        // "Monthly on the first Friday for 10 occurrences"
        let d = dates(
            "FREQ=MONTHLY;COUNT=10;BYDAY=1FR",
            "1997-09-05T09:00",
            "1999-01-01T00:00",
        );
        assert_eq!(&d[..3], ["1997-09-05", "1997-10-03", "1997-11-07"]);
        // "Monthly on the second-to-last Monday of the month for 6 months"
        let d = dates(
            "FREQ=MONTHLY;COUNT=6;BYDAY=-2MO",
            "1997-09-22T09:00",
            "1999-01-01T00:00",
        );
        assert_eq!(&d[..3], ["1997-09-22", "1997-10-20", "1997-11-17"]);
        // "Monthly on the third-to-the-last day of the month, forever"
        let d = dates(
            "FREQ=MONTHLY;BYMONTHDAY=-3",
            "1997-09-28T09:00",
            "1998-03-01T00:00",
        );
        assert_eq!(
            &d[..4],
            ["1997-09-28", "1997-10-29", "1997-11-28", "1997-12-29"]
        );
    }

    #[test]
    fn leap_days_and_short_months() {
        assert_eq!(
            dates(
                "FREQ=YEARLY;COUNT=3",
                "2024-02-29T10:00",
                "2040-01-01T00:00"
            ),
            ["2024-02-29", "2028-02-29", "2032-02-29"]
        );
        assert_eq!(
            dates(
                "FREQ=MONTHLY;COUNT=3",
                "2026-01-31T10:00",
                "2027-01-01T00:00"
            ),
            ["2026-01-31", "2026-03-31", "2026-05-31"]
        );
    }

    #[test]
    fn the_time_of_day_is_kept() {
        let r = Rule::parse("FREQ=WEEKLY").unwrap();
        let all = r.expand(dt("2026-03-23T09:00"), dt("2026-04-30T00:00"), 10);
        assert!(all
            .iter()
            .all(|d| d.time().hour() == 9 && d.time().minute() == 0));
    }

    #[test]
    fn unsupported_rules_name_themselves() {
        for (bad, says) in [
            ("FREQ=HOURLY", "HOURLY"),
            ("FREQ=YEARLY;BYWEEKNO=20", "BYWEEKNO"),
            ("FREQ=WEEKLY;BYDAY=2TU", "ordinal"),
            ("FREQ=DAILY;COUNT=2;UNTIL=20270101", "COUNT and UNTIL"),
        ] {
            assert!(Rule::parse(bad).unwrap_err().contains(says), "{bad}");
        }
    }
}
