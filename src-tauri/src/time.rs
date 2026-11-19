// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Wall-clock times, zones and instants.
//!
//! An event is stored as it was meant: a wall-clock time in a named IANA zone
//! (`2026-11-23T09:00` in `Europe/Istanbul`), or a floating date for all-day
//! events. Instants are computed only when needed -- to show the event in
//! someone else's zone, or to fire a reminder.

use jiff::civil::DateTime;
use jiff::tz::TimeZone;

pub fn zone(name: &str) -> Result<TimeZone, String> {
    TimeZone::get(name).map_err(|_| format!("`{name}` is not a time zone this computer knows"))
}

/// `YYYY-MM-DDTHH:MM` (seconds optional) or, for all-day, `YYYY-MM-DD`.
pub fn parse(text: &str) -> Result<DateTime, String> {
    if text.len() == 10 {
        let d: jiff::civil::Date = text
            .parse()
            .map_err(|_| format!("`{text}` is not a date"))?;
        return Ok(d.to_datetime(jiff::civil::Time::midnight()));
    }
    text.parse::<DateTime>()
        .map_err(|_| format!("`{text}` is not a date and time"))
}

/// `2026-11-23T09:00` -- minutes, which is what a calendar shows.
pub fn format(dt: DateTime) -> String {
    format!("{}T{:02}:{:02}", dt.date(), dt.hour(), dt.minute())
}

/// The same moment, seen from another zone. All-day times are not converted.
pub fn convert(dt: DateTime, from: &TimeZone, to: &TimeZone) -> DateTime {
    dt.to_zoned(from.clone())
        .map_or(dt, |z| z.with_time_zone(to.clone()).datetime())
}

/// Milliseconds since the epoch for a wall-clock time in a zone.
pub fn instant_ms(dt: DateTime, tz: &TimeZone) -> Option<i64> {
    dt.to_zoned(tz.clone())
        .ok()
        .map(|z| z.timestamp().as_millisecond())
}

pub fn now_in(tz: &TimeZone) -> DateTime {
    jiff::Timestamp::now().to_zoned(tz.clone()).datetime()
}

/// Windows time-zone names, as Outlook writes them in TZID, to IANA names
/// (the common ones; CLDR's windowsZones.xml is the source).
pub fn from_windows(name: &str) -> Option<&'static str> {
    Some(match name.trim_matches('"') {
        "Turkey Standard Time" => "Europe/Istanbul",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Romance Standard Time" => "Europe/Paris",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "FLE Standard Time" => "Europe/Kiev",
        "GTB Standard Time" => "Europe/Bucharest",
        "Russian Standard Time" => "Europe/Moscow",
        "Arabian Standard Time" => "Asia/Dubai",
        "Arab Standard Time" => "Asia/Riyadh",
        "Israel Standard Time" => "Asia/Jerusalem",
        "Egypt Standard Time" => "Africa/Cairo",
        "South Africa Standard Time" => "Africa/Johannesburg",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" => "Asia/Shanghai",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "Korea Standard Time" => "Asia/Seoul",
        "Singapore Standard Time" => "Asia/Singapore",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "New Zealand Standard Time" => "Pacific/Auckland",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Atlantic Standard Time" => "America/Halifax",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "Azerbaijan Standard Time" => "Asia/Baku",
        "Iran Standard Time" => "Asia/Tehran",
        "UTC" | "Coordinated Universal Time" => "UTC",
        _ => return None,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_meeting_in_istanbul_seen_from_berlin_across_daylight_saving() {
        let ist = zone("Europe/Istanbul").unwrap();
        let ber = zone("Europe/Berlin").unwrap();
        // Istanbul is UTC+3 all year; Berlin is +1 in winter, +2 in summer.
        assert_eq!(
            format(convert(parse("2026-01-12T09:00").unwrap(), &ist, &ber)),
            "2026-01-12T07:00"
        );
        assert_eq!(
            format(convert(parse("2026-07-13T09:00").unwrap(), &ist, &ber)),
            "2026-07-13T08:00"
        );
    }

    #[test]
    fn parsing_and_windows_names() {
        assert_eq!(format(parse("2026-11-23").unwrap()), "2026-11-23T00:00");
        assert!(parse("23.11.2026").is_err());
        assert!(zone("Mars/Olympus").is_err());
        assert_eq!(
            from_windows("\"Turkey Standard Time\""),
            Some("Europe/Istanbul")
        );
        assert!(zone(from_windows("Pacific Standard Time").unwrap()).is_ok());
    }
}
