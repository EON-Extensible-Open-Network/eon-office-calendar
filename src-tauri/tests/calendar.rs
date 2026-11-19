// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! EON Calendar on the EON Office library, in a home folder of its own: the
//! roadmap's acceptance criteria that a machine can check. Nothing else has
//! to be installed or running.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use eon_calendar::events::Calendar;
use eon_office_core::manifest::Manifest;
use eon_office_core::Home;
use serde_json::{json, Value};
use std::path::PathBuf;

struct TempHome(PathBuf);

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start(name: &str) -> (TempHome, Home, Calendar) {
    let dir = std::env::temp_dir().join(format!("eon-calendar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let home = Home::open(&dir).unwrap();
    let calendar = Calendar::open(&home, None, "Europe/Istanbul").unwrap();
    (TempHome(dir), home, calendar)
}

fn starts(v: &Value) -> Vec<String> {
    v["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            format!(
                "{} {}",
                o["start"].as_str().unwrap(),
                o["title"].as_str().unwrap()
            )
        })
        .collect()
}

#[test]
fn a_weekly_meeting_keeps_its_own_wall_time_across_daylight_saving() {
    let (_dir, _home, cal) = start("dst");
    cal.create(&json!({
        "title": "Standup", "start": "2026-10-19T09:00", "end": "2026-10-19T09:15",
        "tz": "Europe/Istanbul", "rrule": "FREQ=WEEKLY;BYDAY=MO",
    }))
    .unwrap();
    // Istanbul has no daylight saving; Berlin leaves summer time on 25 October.
    let berlin = cal
        .occurrences("2026-10-19", "2026-11-03", "Europe/Berlin")
        .unwrap();
    assert_eq!(
        starts(&berlin),
        vec![
            "2026-10-19T08:00 Standup",
            "2026-10-26T07:00 Standup",
            "2026-11-02T07:00 Standup"
        ]
    );
    let istanbul = cal
        .occurrences("2026-10-19", "2026-11-03", "Europe/Istanbul")
        .unwrap();
    assert!(starts(&istanbul).iter().all(|s| s.contains("T09:00")));
}

#[test]
fn one_occurrence_can_be_deleted_or_changed() {
    let (_dir, _home, cal) = start("exceptions");
    let made = cal
        .create(&json!({
            "title": "Gym", "start": "2026-11-23T18:00", "end": "2026-11-23T19:00",
            "rrule": "FREQ=DAILY;COUNT=5",
        }))
        .unwrap();
    let id = made["id"].as_str().unwrap();
    let rev = made["rev"].as_u64().unwrap();
    cal.delete_occurrence(id, "2026-11-24T18:00", rev).unwrap();
    cal.change_occurrence(
        id,
        "2026-11-25T18:00",
        &json!({ "title": "Gym (late)", "start": "2026-11-25T20:00", "end": "2026-11-25T21:00" }),
    )
    .unwrap();
    let week = cal
        .occurrences("2026-11-23", "2026-11-30", "Europe/Istanbul")
        .unwrap();
    assert_eq!(
        starts(&week),
        vec![
            "2026-11-23T18:00 Gym",
            "2026-11-25T20:00 Gym (late)",
            "2026-11-26T18:00 Gym",
            "2026-11-27T18:00 Gym",
        ]
    );
    // Deleting the changed occurrence deletes that occurrence: the original
    // does not come back in its place.
    let moved = cal
        .change_occurrence(id, "2026-11-26T18:00", &json!({ "title": "Gym (moved)" }))
        .unwrap();
    cal.delete(moved["id"].as_str().unwrap()).unwrap();
    let week = cal
        .occurrences("2026-11-23", "2026-11-30", "Europe/Istanbul")
        .unwrap();
    assert_eq!(
        starts(&week),
        vec![
            "2026-11-23T18:00 Gym",
            "2026-11-25T20:00 Gym (late)",
            "2026-11-27T18:00 Gym"
        ]
    );
    // Deleting the series takes its changed occurrence with it.
    let gone = cal.delete(id).unwrap();
    assert_eq!(gone["deleted"].as_array().unwrap().len(), 2);
    assert_eq!(
        cal.occurrences("2026-11-23", "2026-11-30", "Europe/Istanbul")
            .unwrap()["occurrences"],
        json!([])
    );
}

#[test]
fn all_day_events_and_validation() {
    let (_dir, _home, cal) = start("allday");
    cal.create(
        &json!({ "title": "Holiday", "all_day": true, "start": "2026-12-31", "end": "2027-01-01" }),
    )
    .unwrap();
    let days = cal
        .occurrences("2027-01-01", "2027-01-02", "America/New_York")
        .unwrap();
    assert_eq!(
        starts(&days),
        vec!["2026-12-31 Holiday"],
        "an all-day event is the same days everywhere"
    );
    assert!(cal
        .create(&json!({ "title": "", "start": "2026-11-23T09:00" }))
        .is_err());
    assert!(cal
        .create(&json!({ "title": "x", "start": "2026-11-23T09:00", "end": "2026-11-23T08:00" }))
        .is_err());
}

#[test]
fn reminders_fire_once_without_touching_the_event() {
    let (_dir, home, cal) = start("remind");
    // "Now" is 2026-11-23 08:50 in Istanbul (05:50 UTC).
    let now = cal
        .instant_ms("2026-11-23T08:50", "Europe/Istanbul")
        .unwrap();
    let soon = cal
        .create(&json!({ "title": "Dentist", "location": "Kadıköy", "start": "2026-11-23T09:00", "end": "2026-11-23T09:30", "reminder_minutes": 15 }))
        .unwrap();
    cal.create(&json!({ "title": "Later", "start": "2026-11-23T15:00", "reminder_minutes": 15 }))
        .unwrap();
    cal.create(&json!({ "title": "Over", "start": "2026-11-23T07:00", "end": "2026-11-23T08:00", "reminder_minutes": 10 }))
        .unwrap();
    let daily = cal
        .create(&json!({ "title": "Pills", "start": "2026-11-01T08:45", "rrule": "FREQ=DAILY", "reminder_minutes": 0 }))
        .unwrap();

    let fired = cal.due_reminders(now);
    let titles: Vec<&str> = fired.iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(
        titles,
        vec!["Pills", "Dentist"],
        "in the order they fall due: {fired:?}"
    );
    assert_eq!(fired[1]["body"], "Kadıköy · 09:00");
    assert!(cal.due_reminders(now + 60_000).is_empty(), "once");
    // The event itself is untouched, so a window holding it can still save.
    assert_eq!(
        cal.get(soon["id"].as_str().unwrap()).unwrap()["rev"],
        soon["rev"]
    );
    // Tomorrow's occurrence of the daily event fires again, once.
    let tomorrow = cal
        .instant_ms("2026-11-24T08:46", "Europe/Istanbul")
        .unwrap();
    let next: Vec<Value> = cal.due_reminders(tomorrow);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0]["id"], daily["id"]);
    // Revoking notifications is honoured.
    home.shared()
        .unwrap()
        .revoke(eon_calendar::APP_ID, "notifications")
        .unwrap();
    assert!(!cal.may_notify());
}

#[test]
fn outlook_files_import_and_the_calendar_exports_back() {
    let (_dir, _home, cal) = start("ics");
    let outlook = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:o1\r\nSUMMARY:Bütçe toplantısı\r\nDTSTART;TZID=\"Turkey Standard Time\":20261124T140000\r\nDTEND;TZID=\"Turkey Standard Time\":20261124T153000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEXDATE;TZID=\"Turkey Standard Time\":20261201T140000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:o1\r\nRECURRENCE-ID;TZID=\"Turkey Standard Time\":20261208T140000\r\nSUMMARY:Bütçe (moved)\r\nDTSTART;TZID=\"Turkey Standard Time\":20261208T160000\r\nDTEND;TZID=\"Turkey Standard Time\":20261208T170000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let report = cal.import_ics(outlook).unwrap();
    assert_eq!(
        (report["imported"].clone(), report["skipped"].clone()),
        (json!(2), json!(0))
    );
    let three = cal
        .occurrences("2026-11-24", "2026-12-09", "Europe/Istanbul")
        .unwrap();
    assert_eq!(
        starts(&three),
        vec![
            "2026-11-24T14:00 Bütçe toplantısı",
            "2026-12-08T16:00 Bütçe (moved)"
        ]
    );
    // The same file again, or the calendar's own export: nothing doubles.
    let again = cal.import_ics(outlook).unwrap();
    assert_eq!(
        (again["imported"].clone(), again["known"].clone()),
        (json!(0), json!(2))
    );
    let out = cal.export_ics().unwrap();
    let own = cal.import_ics(out["ics"].as_str().unwrap()).unwrap();
    assert_eq!(
        (own["imported"].clone(), own["known"].clone()),
        (json!(0), json!(2))
    );
    assert_eq!(
        starts(
            &cal.occurrences("2026-11-24", "2026-12-09", "Europe/Istanbul")
                .unwrap()
        ),
        starts(&three)
    );
    // A changed occurrence that is new, for a series already here, joins it.
    let later = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:o1\r\nRECURRENCE-ID;TZID=\"Turkey Standard Time\":20261124T140000\r\nSUMMARY:Bütçe (early)\r\nDTSTART;TZID=\"Turkey Standard Time\":20261124T100000\r\nDTEND;TZID=\"Turkey Standard Time\":20261124T110000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let joined = cal.import_ics(later).unwrap();
    assert_eq!(
        (joined["imported"].clone(), joined["known"].clone()),
        (json!(1), json!(0))
    );
    assert_eq!(
        starts(
            &cal.occurrences("2026-11-24", "2026-12-09", "Europe/Istanbul")
                .unwrap()
        ),
        vec![
            "2026-11-24T10:00 Bütçe (early)",
            "2026-12-08T16:00 Bütçe (moved)"
        ]
    );
    let out = cal.export_ics().unwrap();
    assert_eq!(out["count"], 3);
    let (_dir2, _home2, fresh) = start("ics2");
    fresh.import_ics(out["ics"].as_str().unwrap()).unwrap();
    assert_eq!(
        starts(
            &fresh
                .occurrences("2026-11-24", "2026-12-09", "Europe/Istanbul")
                .unwrap()
        ),
        starts(
            &cal.occurrences("2026-11-24", "2026-12-09", "Europe/Istanbul")
                .unwrap()
        )
    );
}

#[test]
fn attendees_come_from_contacts_when_it_is_installed() {
    let (_dir, home, cal) = start("people");
    assert_eq!(
        cal.people("ay").unwrap()["matches"],
        json!([]),
        "no Contacts, no names, no error"
    );
    // EON Contacts publishes its summaries; Calendar declares call:org.eon.office.contacts.
    let contacts = home
        .app(
            &Manifest::from_json(json!({
                "manifest": 0, "id": "org.eon.office.contacts", "name": "EON Contacts", "version": "0.2.0",
                "api": { "min": 0, "max": 0 }, "capabilities": ["events.publish"],
            }))
            .unwrap(),
            None,
        )
        .unwrap();
    contacts
        .publish("contact:c1", &json!({ "id": "c1", "name": "Ayşe Yılmaz", "org": "Acme", "emails": ["ayse@example.org"], "phones": [] }))
        .unwrap();
    contacts
        .publish(
            "contact:c2",
            &json!({ "id": "c2", "name": "Cem Ak", "org": "", "emails": [], "phones": [] }),
        )
        .unwrap();
    let found = cal.people("AYSE").unwrap();
    assert_eq!(
        found["matches"],
        json!([{ "name": "Ayşe Yılmaz", "email": "ayse@example.org" }])
    );
    assert_eq!(
        cal.people("ayse@example.org").unwrap()["matches"][0]["name"],
        "Ayşe Yılmaz"
    );
}

#[test]
fn a_link_argument_opens_the_event() {
    let (_dir, _home, cal) = start("link");
    let made = cal
        .create(&json!({ "title": "Linked", "start": "2026-11-23T09:00" }))
        .unwrap();
    let id = made["id"].as_str().unwrap();
    assert_eq!(
        cal.link(&format!("eon-office://org.eon.office.calendar/{id}")),
        Some(json!({ "open": id, "date": "2026-11-23T09:00" }))
    );
    assert_eq!(
        cal.link("eon-office://org.eon.office.calendar"),
        Some(json!({ "focus": true }))
    );
    assert_eq!(cal.link("eon-office://org.eon.office.notes/x"), None);
    assert_eq!(cal.link("eon-office://org.eon.office.calendar/nope"), None);
}
