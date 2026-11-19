// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Events as iCalendar `VEVENT` (RFC 5545): out to, and in from, Outlook,
//! Google, Apple, Thunderbird and Nextcloud.
//!
//! Read: DTSTART/DTEND with `TZID` (IANA, or Outlook's Windows names),
//! `Z` (UTC) or `VALUE=DATE` (all day); DURATION; RRULE; EXDATE (several,
//! comma-separated); RECURRENCE-ID (a changed occurrence); VALARM with a
//! relative TRIGGER; SUMMARY, LOCATION, DESCRIPTION, CATEGORIES, UID,
//! ATTENDEE (name and address; roles and replies are not kept). A rule
//! this calendar does not run keeps the event's first occurrence and says why
//! in its notes, rather than repeating it wrongly.

use crate::rrule::{parse_ical_datetime, Rule};
use crate::time;
use serde_json::{json, Value};

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out.push_str("\r\n");
}

fn compact(wall: &str) -> String {
    // 2026-11-23T09:00 -> 20261123T090000; 2026-11-23 -> 20261123
    let digits: String = wall.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 8 {
        digits
    } else {
        format!("{}T{}00", &digits[..8], &digits[8..12.min(digits.len())])
    }
}

fn when(prop: &str, event: &Value, wall: &str) -> String {
    if event["all_day"] == true {
        format!("{prop};VALUE=DATE:{}", compact(wall))
    } else {
        format!(
            "{prop};TZID={}:{}",
            event["tz"].as_str().unwrap_or("UTC"),
            compact(wall)
        )
    }
}

#[allow(clippy::too_many_lines)] // one VEVENT, property by property, reads best whole
pub fn export(events: &[Value], stamp: &str) -> String {
    let mut out = String::new();
    for line in [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//EON//EON Calendar//EN",
        "CALSCALE:GREGORIAN",
    ] {
        fold(line, &mut out);
    }
    let uid = |e: &Value| {
        let own = e["uid"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(str::to_owned);
        own.unwrap_or_else(|| {
            format!(
                "{}@eon-office-calendar",
                e["id"].as_str().unwrap_or_default()
            )
        })
    };
    for e in events {
        fold("BEGIN:VEVENT", &mut out);
        let parent = e["recurrence_of"].as_str().filter(|p| !p.is_empty());
        let my_uid = match parent.and_then(|p| events.iter().find(|x| x["id"] == p)) {
            Some(p) => uid(p),
            None => uid(e),
        };
        fold(&format!("UID:{my_uid}"), &mut out);
        fold(&format!("DTSTAMP:{stamp}"), &mut out);
        fold(
            &when("DTSTART", e, e["start"].as_str().unwrap_or_default()),
            &mut out,
        );
        let end = e["end"].as_str().unwrap_or_default();
        if e["all_day"] == true {
            // DTEND is exclusive for dates: the day after the last day.
            let next = time::parse(end)
                .ok()
                .and_then(|d| d.date().tomorrow().ok())
                .map(|d| d.to_string())
                .unwrap_or_default();
            fold(&format!("DTEND;VALUE=DATE:{}", compact(&next)), &mut out);
        } else {
            fold(&when("DTEND", e, end), &mut out);
        }
        if parent.is_some() {
            fold(
                &when(
                    "RECURRENCE-ID",
                    e,
                    e["recurrence_id"].as_str().unwrap_or_default(),
                ),
                &mut out,
            );
        }
        fold(
            &format!(
                "SUMMARY:{}",
                escape(e["title"].as_str().unwrap_or_default())
            ),
            &mut out,
        );
        for (prop, key) in [("LOCATION", "location"), ("DESCRIPTION", "notes")] {
            if let Some(v) = e[key].as_str().filter(|v| !v.is_empty()) {
                fold(&format!("{prop}:{}", escape(v)), &mut out);
            }
        }
        if let Some(c) = e["calendar"].as_str().filter(|c| !c.is_empty()) {
            fold(&format!("CATEGORIES:{}", escape(c)), &mut out);
        }
        for a in e["attendees"].as_array().into_iter().flatten() {
            let email = a["email"].as_str().unwrap_or_default();
            // A parameter value cannot hold a double quote; quoted, it may hold `;`, `:` and `,`.
            let name: String = a["name"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .filter(|c| *c != '"' && !c.is_control())
                .collect();
            if name.is_empty() {
                fold(&format!("ATTENDEE:mailto:{email}"), &mut out);
            } else {
                fold(&format!("ATTENDEE;CN=\"{name}\":mailto:{email}"), &mut out);
            }
        }
        if let Some(r) = e["rrule"].as_str().filter(|r| !r.is_empty()) {
            fold(&format!("RRULE:{r}"), &mut out);
        }
        for x in e["exdates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            fold(&when("EXDATE", e, x), &mut out);
        }
        if let Some(m) = e["reminder_minutes"].as_i64() {
            for line in [
                "BEGIN:VALARM",
                "ACTION:DISPLAY",
                &format!(
                    "DESCRIPTION:{}",
                    escape(e["title"].as_str().unwrap_or_default())
                ),
                &format!("TRIGGER:-PT{m}M"),
                "END:VALARM",
            ] {
                fold(line, &mut out);
            }
        }
        fold("END:VEVENT", &mut out);
    }
    fold("END:VCALENDAR", &mut out);
    out
}

/// `NAME;PARAM="a:b":value` -> (`NAME;PARAM="a:b"`, `value`): the first colon
/// outside double quotes ends the name and parameters (RFC 5545 3.1).
fn split_property(line: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some((&line[..i], &line[i + 1..])),
            _ => {}
        }
    }
    None
}

/// The name and parameters, split at semicolons outside double quotes.
fn split_params(head: &str) -> Vec<&str> {
    let (mut out, mut quoted, mut from) = (Vec::new(), false, 0);
    for (i, c) in head.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                out.push(&head[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(&head[from..]);
    out
}

/// `TZID` value to an IANA zone this machine knows, if possible.
fn zone_name(tzid: &str) -> Option<String> {
    let t = tzid.trim_matches('"');
    if time::zone(t).is_ok() {
        return Some(t.to_owned());
    }
    time::from_windows(t).map(str::to_owned)
}

/// `-PT15M`, `-P1D`, `-PT1H30M` before the start -> minutes.
fn trigger_minutes(value: &str) -> Option<i64> {
    let v = value.trim();
    let before = v.starts_with('-');
    let v = v.trim_start_matches(['-', '+']).strip_prefix('P')?;
    let (days, rest) = match v.split_once('D') {
        Some((d, rest)) => (d.trim_end_matches('W').parse::<i64>().ok()?, rest),
        None => (0, v),
    };
    let mut minutes = days * 1440;
    if let Some(t) = rest.strip_prefix('T') {
        let mut num = String::new();
        for c in t.chars() {
            match c {
                '0'..='9' => num.push(c),
                'H' => {
                    minutes += num.parse::<i64>().ok()? * 60;
                    num.clear();
                }
                'M' => {
                    minutes += num.parse::<i64>().ok()?;
                    num.clear();
                }
                'S' => num.clear(),
                _ => return None,
            }
        }
    }
    before.then_some(minutes).or(Some(0))
}

/// Duration like `PT1H30M` or `P1D` -> minutes.
fn duration_minutes(value: &str) -> Option<i64> {
    trigger_minutes(&format!("-{}", value.trim_start_matches('+')))
}

/// Every VEVENT in `text`, as event fields in `default_tz` where the file has
/// no zone. Changed occurrences carry `parent_uid` and `recurrence_id`.
pub fn import(text: &str, default_tz: &str) -> Vec<Value> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        lines.push(raw.to_owned());
    }
    let mut out = Vec::new();
    let mut ev: Option<Value> = None;
    let mut in_alarm = false;
    for line in lines {
        let Some((head, value)) = split_property(&line) else {
            continue;
        };
        let mut parts = split_params(head).into_iter();
        let name = parts.next().unwrap_or_default().to_ascii_uppercase();
        let params: Vec<(String, String)> = parts
            .filter_map(|p| {
                p.split_once('=')
                    .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_owned()))
            })
            .collect();
        let param = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        match (name.as_str(), ev.as_mut()) {
            ("BEGIN", None) if value.eq_ignore_ascii_case("VEVENT") => {
                ev = Some(json!({ "exdates": [] }));
            }
            ("BEGIN", Some(_)) if value.eq_ignore_ascii_case("VALARM") => in_alarm = true,
            ("END", Some(_)) if value.eq_ignore_ascii_case("VALARM") => in_alarm = false,
            ("TRIGGER", Some(e)) if in_alarm => {
                if param("VALUE").is_none() {
                    e["reminder_minutes"] = json!(trigger_minutes(value));
                }
            }
            (_, Some(_)) if in_alarm => {}
            ("END", Some(_)) if value.eq_ignore_ascii_case("VEVENT") => {
                if let Some(mut e) = ev.take() {
                    finish(&mut e, default_tz);
                    if e.get("title").is_some() && e.get("start").is_some() {
                        out.push(e);
                    }
                }
            }
            ("UID", Some(e)) => e["uid"] = json!(value),
            ("SUMMARY", Some(e)) => e["title"] = json!(unescape(value)),
            ("LOCATION", Some(e)) => e["location"] = json!(unescape(value)),
            ("DESCRIPTION", Some(e)) => e["notes"] = json!(unescape(value)),
            ("CATEGORIES", Some(e)) => {
                e["calendar"] = json!(unescape(value.split(',').next().unwrap_or_default()));
            }
            ("RRULE", Some(e)) => e["rrule"] = json!(value),
            ("ATTENDEE", Some(e)) => {
                let email = value
                    .get(..7)
                    .filter(|p| p.eq_ignore_ascii_case("mailto:"))
                    .map_or(value, |_| &value[7..]);
                let entry = json!({ "name": param("CN").unwrap_or_default(), "email": email });
                match e["attendees"].as_array_mut() {
                    Some(list) => list.push(entry),
                    None => e["attendees"] = json!([entry]),
                }
            }
            ("DURATION", Some(e)) => e["duration"] = json!(duration_minutes(value)),
            ("DTSTART" | "DTEND" | "RECURRENCE-ID", Some(e)) => {
                let key = match name.as_str() {
                    "DTSTART" => "start",
                    "DTEND" => "end",
                    _ => "recurrence_id",
                };
                e[format!("{key}_raw")] = json!({ "value": value, "tzid": param("TZID"), "date": param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE")) });
            }
            ("EXDATE", Some(e)) => {
                if let Some(list) = e["exdates_raw"].as_array_mut() {
                    for v in value.split(',') {
                        list.push(json!({ "value": v, "tzid": param("TZID") }));
                    }
                } else {
                    e["exdates_raw"] = json!(value
                        .split(',')
                        .map(|v| json!({ "value": v, "tzid": param("TZID") }))
                        .collect::<Vec<_>>());
                }
            }
            _ => {}
        }
    }
    out
}

/// Turn the raw times into wall-clock times in one zone.
#[allow(clippy::too_many_lines)] // the raw-to-wall conversion reads best in one place
fn finish(e: &mut Value, default_tz: &str) {
    let tz_of = |raw: &Value| -> String {
        raw["tzid"].as_str().and_then(zone_name).unwrap_or_else(|| {
            if raw["value"].as_str().is_some_and(|v| v.ends_with('Z')) {
                "UTC".into()
            } else {
                default_tz.into()
            }
        })
    };
    let start_raw = e["start_raw"].clone();
    if start_raw.is_null() {
        return;
    }
    let all_day = start_raw["date"] == true;
    let tz = if all_day {
        default_tz.to_owned()
    } else {
        tz_of(&start_raw)
    };
    let wall = |raw: &Value| -> Option<String> {
        let v = raw["value"].as_str()?;
        if raw["date"] == true || (v.len() == 8 && all_day) {
            let dt = parse_ical_datetime(&v[..8])?;
            return Some(dt.date().to_string());
        }
        let dt = parse_ical_datetime(v)?;
        // A time in another zone than the start's (rare) is moved into the start's zone.
        let from = time::zone(&tz_of(raw)).ok()?;
        let to = time::zone(&tz).ok()?;
        Some(time::format(time::convert(dt, &from, &to)))
    };
    let Some(start) = wall(&start_raw) else {
        return;
    };
    e["start"] = json!(start);
    e["tz"] = json!(tz);
    e["all_day"] = json!(all_day);
    let end = if let Some(end) = wall(&e["end_raw"]) {
        if all_day {
            // DTEND;VALUE=DATE is exclusive: the event ends the day before.
            time::parse(&end)
                .ok()
                .and_then(|d| d.date().yesterday().ok())
                .map(|d| d.to_string())
                .filter(|d| d.as_str() >= start.as_str())
                .unwrap_or_else(|| start.clone())
        } else {
            end
        }
    } else if let Some(m) = e["duration"].as_i64() {
        time::parse(&start)
            .ok()
            .and_then(|s| s.checked_add(jiff::Span::new().minutes(m)).ok())
            .map_or_else(
                || start.clone(),
                |d| {
                    if all_day {
                        d.date().to_string()
                    } else {
                        time::format(d)
                    }
                },
            )
    } else {
        start.clone()
    };
    e["end"] = json!(end);
    if let Some(id) = wall(&e["recurrence_id_raw"]) {
        e["recurrence_id"] = json!(id);
        e["parent_uid"] = e["uid"].clone();
    }
    let ex: Vec<Value> = e["exdates_raw"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|x| wall(&json!({ "value": x["value"], "tzid": x["tzid"], "date": all_day })))
        .map(Value::from)
        .collect();
    e["exdates"] = json!(ex);
    if let Some(rule) = e["rrule"]
        .as_str()
        .map(str::to_owned)
        .filter(|r| !r.is_empty())
    {
        if let Err(why) = Rule::parse(&rule) {
            let note = e["notes"].as_str().unwrap_or_default().to_owned();
            e["notes"] = json!(format!(
                "{note}{}Imported repetition not supported here ({why}): {rule}",
                if note.is_empty() { "" } else { "\n\n" }
            ));
            e["rrule"] = json!("");
        }
    }
    if let Some(o) = e.as_object_mut() {
        for k in [
            "start_raw",
            "end_raw",
            "recurrence_id_raw",
            "exdates_raw",
            "duration",
        ] {
            o.remove(k);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn events_survive_a_round_trip() {
        let events = vec![
            json!({ "id": "e1", "title": "Standup; daily", "location": "Room 3", "notes": "a\nb", "calendar": "Work", "all_day": false,
                "start": "2026-11-23T09:00", "end": "2026-11-23T09:15", "tz": "Europe/Istanbul", "rrule": "FREQ=WEEKLY;BYDAY=MO,WE,FR",
                "exdates": ["2026-11-25T09:00"], "reminder_minutes": 10, "recurrence_of": "", "recurrence_id": "",
                "attendees": [{ "name": "Ayşe Yılmaz", "email": "ayse@example.org" }, { "name": "", "email": "ops@example.org" }] }),
            json!({ "id": "e2", "title": "Holiday", "all_day": true, "start": "2026-12-31", "end": "2027-01-01", "tz": "Europe/Istanbul", "recurrence_of": "" }),
            json!({ "id": "e3", "title": "Standup moved", "all_day": false, "start": "2026-11-27T11:00", "end": "2026-11-27T11:15", "tz": "Europe/Istanbul", "recurrence_of": "e1", "recurrence_id": "2026-11-27T09:00" }),
        ];
        let text = export(&events, "20261119T090000Z");
        assert!(text.lines().all(|l| l.len() <= 76));
        let back = import(&text, "UTC");
        assert_eq!(back.len(), 3);
        let e = &back[0];
        assert_eq!(
            (
                e["title"].clone(),
                e["start"].clone(),
                e["end"].clone(),
                e["tz"].clone()
            ),
            (
                json!("Standup; daily"),
                json!("2026-11-23T09:00"),
                json!("2026-11-23T09:15"),
                json!("Europe/Istanbul")
            )
        );
        assert_eq!(
            (
                e["rrule"].clone(),
                e["exdates"].clone(),
                e["reminder_minutes"].clone()
            ),
            (
                json!("FREQ=WEEKLY;BYDAY=MO,WE,FR"),
                json!(["2026-11-25T09:00"]),
                json!(10)
            )
        );
        assert_eq!(
            e["attendees"],
            json!([{ "name": "Ayşe Yılmaz", "email": "ayse@example.org" }, { "name": "", "email": "ops@example.org" }])
        );
        assert!(text.contains("ATTENDEE;CN=\"Ayşe Yılmaz\":mailto:ayse@example.org"));
        assert_eq!(
            (
                back[1]["start"].clone(),
                back[1]["end"].clone(),
                back[1]["all_day"].clone()
            ),
            (json!("2026-12-31"), json!("2027-01-01"), json!(true))
        );
        assert_eq!(
            (
                back[2]["parent_uid"].clone(),
                back[2]["recurrence_id"].clone()
            ),
            (back[0]["uid"].clone(), json!("2026-11-27T09:00"))
        );
    }

    #[test]
    fn outlook_google_and_apple_files_are_read() {
        // Outlook: a Windows zone name; Google: UTC with Z and DURATION; Apple: VALUE=DATE.
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Turkey Standard Time\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:o1\r\nSUMMARY:Bütçe toplantısı\r\nDTSTART;TZID=\"Turkey Standard Time\":20261124T140000\r\nDTEND;TZID=\"Turkey Standard Time\":20261124T153000\r\nRRULE:FREQ=MONTHLY;BYDAY=4TU\r\nEXDATE;TZID=\"Turkey Standard Time\":20261222T140000,20270126T140000\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:g1@google.com\r\nSUMMARY:Call\r\nDTSTART:20261125T120000Z\r\nDURATION:PT45M\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a1\r\nSUMMARY:Republic Day\r\nDTSTART;VALUE=DATE:20261029\r\nDTEND;VALUE=DATE:20261030\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:x1\r\nSUMMARY:Odd\r\nDTSTART:20261125T120000Z\r\nRRULE:FREQ=MINUTELY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = import(text, "Europe/Istanbul");
        assert_eq!(ev.len(), 4);
        assert_eq!(
            (
                ev[0]["tz"].clone(),
                ev[0]["start"].clone(),
                ev[0]["end"].clone()
            ),
            (
                json!("Europe/Istanbul"),
                json!("2026-11-24T14:00"),
                json!("2026-11-24T15:30")
            )
        );
        assert_eq!(
            (
                ev[0]["exdates"].as_array().map(Vec::len),
                ev[0]["reminder_minutes"].clone()
            ),
            (Some(2), json!(15))
        );
        assert_eq!(
            (
                ev[1]["tz"].clone(),
                ev[1]["start"].clone(),
                ev[1]["end"].clone()
            ),
            (
                json!("UTC"),
                json!("2026-11-25T12:00"),
                json!("2026-11-25T12:45")
            )
        );
        assert_eq!(
            (
                ev[2]["all_day"].clone(),
                ev[2]["start"].clone(),
                ev[2]["end"].clone()
            ),
            (json!(true), json!("2026-10-29"), json!("2026-10-29"))
        );
        assert_eq!(ev[3]["rrule"], "");
        assert!(
            ev[3]["notes"].as_str().unwrap().contains("MINUTELY"),
            "an unsupported rule is kept as a note, not lost"
        );
    }

    #[test]
    fn triggers_and_durations() {
        assert_eq!(trigger_minutes("-PT15M"), Some(15));
        assert_eq!(trigger_minutes("-P1DT2H"), Some(1560));
        assert_eq!(duration_minutes("PT1H30M"), Some(90));
        assert_eq!(duration_minutes("P1D"), Some(1440));
    }
}
