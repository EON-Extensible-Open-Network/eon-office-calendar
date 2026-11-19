// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Events, kept with the EON Office library.
//!
//! An event: `{ title, location, notes, calendar, all_day, start, end, tz,
//! rrule, exdates, recurrence_of, recurrence_id, reminder_minutes, uid,
//! attendees }`, an attendee being `{ name, email }`.
//! `start`/`end` are wall-clock times in `tz`. A repeating event's single
//! occurrence can be deleted (its original start goes to `exdates`) or
//! changed (an override event with `recurrence_of` and `recurrence_id`
//! replaces that occurrence), exactly as RFC 5545 models it.
//!
//! Reminders are the window's own: it asks [`Calendar::due_reminders`] every
//! few seconds. Which occurrence was last shown lives in a separate `shown`
//! collection, never in the event, so a reminder never makes a window that
//! is editing the event lose its save.

use crate::rrule::Rule;
use crate::time;
use eon_office_core::manifest::{Capability, Manifest};
use eon_office_core::{links as eon_links, AppContext, Home};
use jiff::civil::DateTime;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

pub const COLLECTION: &str = "events";
/// The last occurrence whose reminder was shown, per event.
const SHOWN: &str = "shown";
const CONTACTS: &str = "org.eon.office.contacts";
pub type Outcome = Result<Value, Value>;
const MAX_OCCURRENCES: usize = 5000;
/// The longest reminder lead: four weeks.
const MAX_LEAD_MINUTES: i64 = 40_320;

fn fail(code: &str, message: impl Into<String>) -> Value {
    json!({ "code": code, "message": message.into() })
}

/// A library error as the interface sees it: `{ code, message, data }`.
pub fn core<T>(result: eon_office_core::Result<T>) -> Result<T, Value> {
    result.map_err(|e| {
        let error = e.to_json();
        json!({ "code": error["data"]["code"], "message": error["message"], "data": error["data"] })
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

static COUNTER: AtomicU64 = AtomicU64::new(0);
fn new_id() -> String {
    format!(
        "e{:x}{:x}",
        now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed) % 4096
    )
}

/// Search-style folding, for matching attendees.
fn fold(text: &str) -> String {
    eon_office_core::text::fold(text.trim())
}

/// Checked, normalised event fields.
pub fn clean(fields: &Value, default_tz: &str) -> Result<Value, Value> {
    let bad = |m: String| fail("invalid_params", m);
    let title = fields["title"].as_str().unwrap_or_default().trim();
    if title.is_empty() || title.chars().count() > 300 {
        return Err(bad("a title is 1-300 characters".into()));
    }
    let all_day = fields["all_day"].as_bool().unwrap_or(false);
    let tz = fields["tz"]
        .as_str()
        .filter(|t| !t.is_empty())
        .unwrap_or(default_tz);
    time::zone(tz).map_err(bad)?;
    let start = time::parse(fields["start"].as_str().unwrap_or_default()).map_err(bad)?;
    let end = match fields["end"].as_str().filter(|e| !e.is_empty()) {
        Some(e) => time::parse(e).map_err(bad)?,
        None if all_day => start,
        None => start
            .checked_add(jiff::Span::new().hours(1))
            .map_err(|e| bad(e.to_string()))?,
    };
    if end < start {
        return Err(bad("an event cannot end before it starts".into()));
    }
    let rule = fields["rrule"].as_str().unwrap_or_default();
    if !rule.is_empty() {
        Rule::parse(rule).map_err(bad)?;
    }
    let reminder = fields["reminder_minutes"]
        .as_i64()
        .filter(|m| (0..=MAX_LEAD_MINUTES).contains(m));
    let fmt = |d: DateTime| {
        if all_day {
            d.date().to_string()
        } else {
            time::format(d)
        }
    };
    let exdates: Vec<String> = fields["exdates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().and_then(|s| time::parse(s).ok()).map(fmt))
        .collect();
    // People invited: a name (may be empty) and an address, one per address.
    let mut seen = BTreeSet::new();
    let attendees: Vec<Value> = fields["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let email = a["email"].as_str()?.trim();
            let name: String = a["name"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .chars()
                .take(200)
                .collect();
            let valid = email.len() <= 254
                && email
                    .split_once('@')
                    .is_some_and(|(u, d)| !u.is_empty() && !d.is_empty());
            (valid && seen.insert(fold(email))).then(|| json!({ "name": name, "email": email }))
        })
        .take(500)
        .collect();
    let calendar = fields["calendar"]
        .as_str()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or("Personal");
    Ok(json!({
        "title": title, "location": fields["location"].as_str().unwrap_or_default().trim(),
        "notes": fields["notes"].as_str().unwrap_or_default(),
        "calendar": calendar,
        "all_day": all_day, "start": fmt(start), "end": fmt(end), "tz": tz,
        "rrule": rule, "exdates": exdates,
        "recurrence_of": fields["recurrence_of"].as_str().unwrap_or_default(),
        "recurrence_id": fields["recurrence_id"].as_str().unwrap_or_default(),
        "reminder_minutes": reminder, "uid": fields["uid"].as_str().unwrap_or_default(),
        "attendees": attendees,
    }))
}

pub struct Calendar {
    ctx: Mutex<AppContext>,
    /// The zone of the machine the window runs on: new events default to it
    /// and the window shows occurrences in it.
    pub local_tz: String,
}

impl Calendar {
    /// Open the calendar's store in `home`, registering it with `folder` as
    /// where its program lives (`None`: leave the registration as it is).
    pub fn open(home: &Home, folder: Option<&Path>, local_tz: &str) -> Result<Calendar, Value> {
        let manifest: Value = serde_json::from_str(crate::MANIFEST)
            .map_err(|e| fail("internal", format!("manifest.json: {e}")))?;
        let manifest = core(Manifest::from_json(manifest))?;
        Ok(Calendar {
            ctx: Mutex::new(core(home.app(&manifest, folder))?),
            local_tz: local_tz.to_owned(),
        })
    }

    fn ctx(&self) -> MutexGuard<'_, AppContext> {
        self.ctx.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The person's preferences: language, theme, text size, motion, contrast.
    pub fn prefs(&self) -> Outcome {
        core(self.ctx().prefs())
    }

    /// Changes whenever a preference changes; the window polls it.
    pub fn prefs_version(&self) -> i64 {
        self.ctx().prefs_version().unwrap_or(0)
    }

    /// Declared and not revoked: reminders may become notifications.
    pub fn may_notify(&self) -> bool {
        self.ctx().allows(&Capability::Notifications)
    }

    /// A wall-clock time in a zone, as milliseconds since the epoch.
    pub fn instant_ms(&self, wall: &str, tz: &str) -> Result<u64, Value> {
        let zone = time::zone(tz).map_err(|e| fail("invalid_params", e))?;
        let wall = time::parse(wall).map_err(|e| fail("invalid_params", e))?;
        time::instant_ms(wall, &zone)
            .and_then(|ms| u64::try_from(ms).ok())
            .ok_or_else(|| fail("invalid_params", "not a time this zone has"))
    }

    fn all(&self) -> Result<Vec<Value>, Value> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = core(self.ctx().list(COLLECTION, after.as_deref(), 1000))?;
            let items = page["items"].as_array().cloned().unwrap_or_default();
            for item in &items {
                let mut e = item["doc"].clone();
                e["id"] = item["id"].clone();
                e["rev"] = item["rev"].clone();
                out.push(e);
            }
            if items.len() < 1000 {
                break;
            }
            after = items
                .last()
                .and_then(|i| i["id"].as_str())
                .map(str::to_owned);
        }
        Ok(out)
    }

    /// Every occurrence overlapping `[from, to)` (wall-clock in `viewer_tz`),
    /// in the viewer's zone, sorted, all-day first. Exceptions and changed
    /// occurrences applied. Also the calendars there are, with counts.
    #[allow(clippy::too_many_lines)] // one pass over the events reads best whole
    pub fn occurrences(&self, from: &str, to: &str, viewer_tz: &str) -> Outcome {
        let bad = |e: String| fail("invalid_params", e);
        let viewer = time::zone(viewer_tz).map_err(bad)?;
        let (from, to) = (
            time::parse(from).map_err(bad)?,
            time::parse(to).map_err(bad)?,
        );
        let all = self.all()?;
        let overridden: BTreeSet<(String, String)> = all
            .iter()
            .filter(|e| e["recurrence_of"].as_str().is_some_and(|r| !r.is_empty()))
            .map(|e| {
                (
                    e["recurrence_of"].as_str().unwrap_or_default().to_owned(),
                    e["recurrence_id"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let mut calendars: BTreeMap<String, usize> = BTreeMap::new();
        let mut out = Vec::new();
        for event in &all {
            *calendars
                .entry(event["calendar"].as_str().unwrap_or("Personal").to_owned())
                .or_default() += 1;
            let all_day = event["all_day"] == true;
            let Ok(tz) = time::zone(event["tz"].as_str().unwrap_or("UTC")) else {
                continue;
            };
            let (Ok(start), Ok(end)) = (
                time::parse(event["start"].as_str().unwrap_or_default()),
                time::parse(event["end"].as_str().unwrap_or_default()),
            ) else {
                continue;
            };
            // All-day events are inclusive of their last day.
            let length = if all_day {
                end.checked_add(jiff::Span::new().days(1))
                    .unwrap_or(end)
                    .duration_since(start)
            } else {
                end.duration_since(start)
            };
            // The query range seen from the event's zone, with a day of margin.
            let (lo, hi) = if all_day {
                (from, to)
            } else {
                (
                    time::convert(from, &viewer, &tz),
                    time::convert(to, &viewer, &tz),
                )
            };
            let lo = lo.checked_sub(jiff::Span::new().days(1)).unwrap_or(lo);
            let hi = hi.checked_add(jiff::Span::new().days(1)).unwrap_or(hi);
            let id = event["id"].as_str().unwrap_or_default();
            let starts: Vec<DateTime> = match event["rrule"]
                .as_str()
                .filter(|r| !r.is_empty())
                .and_then(|r| Rule::parse(r).ok())
            {
                Some(rule) => rule.expand(start, hi, MAX_OCCURRENCES),
                None => vec![start],
            };
            let exdates: BTreeSet<&str> = event["exdates"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            for s in starts {
                let key = if all_day {
                    s.date().to_string()
                } else {
                    time::format(s)
                };
                let e = s.checked_add(length).unwrap_or(s);
                if e < lo
                    || exdates.contains(key.as_str())
                    || overridden.contains(&(id.to_owned(), key.clone()))
                {
                    continue;
                }
                let (vs, ve) = if all_day {
                    (s, e)
                } else {
                    (
                        time::convert(s, &tz, &viewer),
                        time::convert(e, &tz, &viewer),
                    )
                };
                // Overlaps [from, to); a zero-length event at `from` counts.
                if vs >= to || ve < from || (ve == from && vs != from) {
                    continue;
                }
                let shown_end = if all_day {
                    end.date().to_string()
                } else {
                    time::format(ve)
                };
                out.push(json!({
                    "id": id, "occurrence": key, "title": event["title"], "location": event["location"],
                    "calendar": event["calendar"], "all_day": all_day,
                    "start": if all_day { vs.date().to_string() } else { time::format(vs) },
                    "end": if all_day && s == start { shown_end } else if all_day { e.date().yesterday().map(|d| d.to_string()).unwrap_or_default() } else { time::format(ve) },
                    "repeats": !event["rrule"].as_str().unwrap_or_default().is_empty(),
                    "changed": !event["recurrence_of"].as_str().unwrap_or_default().is_empty(),
                    "reminder_minutes": event["reminder_minutes"],
                    "tz": event["tz"],
                }));
            }
        }
        out.sort_by(|a, b| {
            (a["start"].as_str(), a["all_day"] != true)
                .cmp(&(b["start"].as_str(), b["all_day"] != true))
        });
        Ok(json!({ "occurrences": out, "calendars": calendars }))
    }

    fn find(&self, id: &str) -> Result<Option<Value>, Value> {
        let found = core(self.ctx().get(COLLECTION, id))?;
        Ok((!found.is_null()).then_some(found))
    }

    pub fn get(&self, id: &str) -> Outcome {
        let found = self
            .find(id)?
            .ok_or_else(|| fail("not_found", "no such event"))?;
        let mut e = found["doc"].clone();
        e["id"] = json!(id);
        e["rev"] = found["rev"].clone();
        Ok(e)
    }

    fn write(&self, id: &str, event: &Value, rev: Option<u64>) -> Outcome {
        let written = core(self.ctx().put(COLLECTION, id, event, rev))?;
        let text = format!(
            "{} {} {}",
            event["location"].as_str().unwrap_or_default(),
            event["notes"].as_str().unwrap_or_default(),
            event["calendar"].as_str().unwrap_or_default()
        );
        let _ = self.ctx().index(
            id,
            event["title"].as_str().unwrap_or_default(),
            &text,
            false,
        );
        Ok(json!({ "id": id, "rev": written["rev"] }))
    }

    fn activity(&self, id: &str, title: &Value, kind: &str) {
        let _ = self
            .ctx()
            .activity(id, title.as_str().unwrap_or_default(), kind);
    }

    pub fn create(&self, fields: &Value) -> Outcome {
        let mut event = clean(fields, &self.local_tz)?;
        event["created"] = json!(now_ms());
        let id = new_id();
        let out = self.write(&id, &event, Some(0))?;
        self.activity(&id, &event["title"], "created");
        Ok(out)
    }

    /// Change some fields; the rest stay. Refused if `rev` is out of date.
    pub fn update(&self, id: &str, fields: &Value, rev: u64) -> Outcome {
        let existing = self.get(id)?;
        let mut merged = existing.clone();
        if let (Some(target), Some(changes)) = (merged.as_object_mut(), fields.as_object()) {
            for (k, v) in changes {
                target.insert(k.clone(), v.clone());
            }
        }
        let mut event = clean(&merged, &self.local_tz)?;
        event["created"] = existing["created"].clone();
        let out = self.write(id, &event, Some(rev))?;
        self.activity(id, &event["title"], "edited");
        Ok(out)
    }

    /// Delete an event; its changed occurrences go with it. Deleting a changed
    /// occurrence deletes that occurrence: the series is told to skip it, or
    /// the original would come back in its place.
    pub fn delete(&self, id: &str) -> Outcome {
        if let Ok(event) = self.get(id) {
            let parent = event["recurrence_of"].as_str().unwrap_or_default();
            let original = event["recurrence_id"].as_str().unwrap_or_default();
            if !parent.is_empty() && !original.is_empty() {
                if let Ok(series) = self.get(parent) {
                    let rev = series["rev"].as_u64().unwrap_or(0);
                    self.delete_occurrence(parent, original, rev)?;
                }
            }
        }
        let mut removed = Vec::new();
        for e in self.all()? {
            let eid = e["id"].as_str().unwrap_or_default().to_owned();
            if eid == id || e["recurrence_of"] == id {
                let _ = self.ctx().delete(COLLECTION, &eid, None);
                let _ = self.ctx().delete(SHOWN, &eid, None);
                let _ = self.ctx().unindex(&eid);
                removed.push(eid);
            }
        }
        if removed.is_empty() {
            return Err(fail("not_found", "no such event"));
        }
        Ok(json!({ "deleted": removed }))
    }

    /// Delete one occurrence of a repeating event.
    pub fn delete_occurrence(&self, id: &str, occurrence: &str, rev: u64) -> Outcome {
        let event = self.get(id)?;
        let mut exdates: Vec<Value> = event["exdates"].as_array().cloned().unwrap_or_default();
        exdates.push(json!(occurrence));
        self.update(id, &json!({ "exdates": exdates }), rev)
    }

    /// Change one occurrence: an event of its own replaces it, RFC 5545 style.
    pub fn change_occurrence(&self, id: &str, occurrence: &str, changes: &Value) -> Outcome {
        let parent = self.get(id)?;
        let mut fields = Map::new();
        for key in [
            "title",
            "location",
            "notes",
            "calendar",
            "all_day",
            "tz",
            "reminder_minutes",
            "attendees",
        ] {
            fields.insert(key.into(), parent[key].clone());
        }
        let length = match (
            time::parse(parent["start"].as_str().unwrap_or_default()),
            time::parse(parent["end"].as_str().unwrap_or_default()),
        ) {
            (Ok(s), Ok(e)) => e.duration_since(s),
            _ => jiff::SignedDuration::from_hours(1),
        };
        let start = time::parse(occurrence).map_err(|e| fail("invalid_params", e))?;
        let end = start.checked_add(length).unwrap_or(start);
        let all_day = parent["all_day"] == true;
        fields.insert("start".into(), json!(occurrence));
        fields.insert(
            "end".into(),
            json!(if all_day {
                end.date().to_string()
            } else {
                time::format(end)
            }),
        );
        if let Some(c) = changes.as_object() {
            for (k, v) in c {
                fields.insert(k.clone(), v.clone());
            }
        }
        fields.insert("recurrence_of".into(), json!(id));
        fields.insert("recurrence_id".into(), json!(occurrence));
        fields.insert("rrule".into(), json!(""));
        fields.insert("exdates".into(), json!([]));
        self.create(&Value::Object(fields))
    }

    /// Reminders due at `now` (ms): an occurrence whose reminder time has come
    /// and which has not ended. Each occurrence fires once -- also one that
    /// fell due while the calendar was closed, if its event is still on.
    /// Which occurrence was shown is kept apart from the event.
    pub fn due_reminders(&self, now: u64) -> Vec<Value> {
        let Ok(now_i) = i64::try_from(now) else {
            return Vec::new();
        };
        let Ok(utc) = time::zone("UTC") else {
            return Vec::new();
        };
        let Some(now_wall) = jiff::Timestamp::from_millisecond(now_i)
            .ok()
            .map(|t| t.to_zoned(utc.clone()).datetime())
        else {
            return Vec::new();
        };
        let from = now_wall
            .checked_sub(jiff::Span::new().days(2))
            .unwrap_or(now_wall);
        let to = now_wall
            .checked_add(jiff::Span::new().minutes(MAX_LEAD_MINUTES).days(1))
            .unwrap_or(now_wall);
        let Ok(window) = self.occurrences(&time::format(from), &time::format(to), "UTC") else {
            return Vec::new();
        };
        // The latest due occurrence per event.
        let mut due: BTreeMap<String, Value> = BTreeMap::new();
        for o in window["occurrences"].as_array().into_iter().flatten() {
            let Some(lead) = o["reminder_minutes"].as_i64() else {
                continue;
            };
            let all_day = o["all_day"] == true;
            let wall = |v: &Value| time::parse(v.as_str().unwrap_or_default()).ok();
            let (Some(start), Some(end)) = (wall(&o["start"]), wall(&o["end"])) else {
                continue;
            };
            // All-day events: reminders count from 09:00 of the day, in the
            // event's own zone (UTC when unknown).
            let (start_ms, end_ms) = if all_day {
                let zone = time::zone(o["tz"].as_str().unwrap_or("UTC")).unwrap_or(utc.clone());
                let at = start.date().at(9, 0, 0, 0);
                let last = end.date().at(23, 59, 0, 0);
                (time::instant_ms(at, &zone), time::instant_ms(last, &zone))
            } else {
                (time::instant_ms(start, &utc), time::instant_ms(end, &utc))
            };
            let (Some(start_ms), Some(end_ms)) = (start_ms, end_ms) else {
                continue;
            };
            let alarm = start_ms - lead * 60_000;
            if alarm > now_i || end_ms <= now_i {
                continue;
            }
            let id = o["id"].as_str().unwrap_or_default().to_owned();
            let key = o["occurrence"].as_str().unwrap_or_default().to_owned();
            let later = due
                .get(&id)
                .is_none_or(|d| d["occurrence"].as_str().unwrap_or_default() < key.as_str());
            if later {
                due.insert(id, o.clone());
            }
        }
        let mut out = Vec::new();
        for (id, o) in due {
            let key = o["occurrence"].as_str().unwrap_or_default().to_owned();
            let shown = self.ctx().get(SHOWN, &id).unwrap_or(Value::Null);
            if shown["doc"]["occurrence"]
                .as_str()
                .is_some_and(|s| s >= key.as_str())
            {
                continue;
            }
            let rev = shown["rev"].as_u64().unwrap_or(0);
            if self
                .ctx()
                .put(SHOWN, &id, &json!({ "occurrence": key }), Some(rev))
                .is_err()
            {
                continue;
            }
            // The time as the person reads it: in the event's own zone.
            let when = self
                .get(&id)
                .ok()
                .and_then(|e| {
                    if o["all_day"] == true {
                        return Some(String::new());
                    }
                    let zone = time::zone(e["tz"].as_str()?).ok()?;
                    let start = time::parse(o["start"].as_str()?).ok()?;
                    let local = time::convert(start, &utc, &zone);
                    Some(format!("{:02}:{:02}", local.hour(), local.minute()))
                })
                .unwrap_or_default();
            let body = [o["location"].as_str().unwrap_or_default(), when.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            out.push(json!({ "id": id, "title": o["title"], "body": body, "occurrence": key }));
        }
        out.sort_by(|a, b| a["occurrence"].as_str().cmp(&b["occurrence"].as_str()));
        out
    }

    /// People to invite, from what EON Contacts publishes: name and e-mail.
    /// Contacts not installed, or the person revoked the permission: no
    /// names, and no error -- typing an address still works.
    pub fn people(&self, query: &str) -> Outcome {
        let q = fold(query);
        if q.is_empty() {
            return Ok(json!({ "matches": [] }));
        }
        let published = self.ctx().public_of(CONTACTS).unwrap_or_default();
        let mut matches = Vec::new();
        for (_, c) in published {
            let name = c["name"].as_str().unwrap_or_default();
            for email in c["emails"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if fold(name).contains(&q) || fold(email) == q || fold(email).starts_with(&q) {
                    matches.push(json!({ "name": name, "email": email }));
                }
            }
        }
        matches.truncate(20);
        Ok(json!({ "matches": matches }))
    }

    /// Every event as iCalendar.
    pub fn export_ics(&self) -> Outcome {
        let events = self.all()?;
        let stamp = jiff::Timestamp::now()
            .strftime("%Y%m%dT%H%M%SZ")
            .to_string();
        Ok(json!({ "ics": crate::ics::export(&events, &stamp), "count": events.len() }))
    }

    /// Import VEVENTs. Changed occurrences attach to the series they belong
    /// to. An event already here -- the same UID, or this calendar's own
    /// export of it -- is counted as `known` and left as it is, so importing a
    /// file twice doubles nothing.
    pub fn import_ics(&self, text: &str) -> Outcome {
        let items = crate::ics::import(text, &self.local_tz);
        // Series by UID: their own, or the one an export gave them.
        let mut ids: BTreeMap<String, String> = BTreeMap::new();
        let mut overridden: BTreeSet<(String, String)> = BTreeSet::new();
        for e in self.all()? {
            let id = e["id"].as_str().unwrap_or_default().to_owned();
            let parent = e["recurrence_of"].as_str().unwrap_or_default();
            if parent.is_empty() {
                let uid = e["uid"]
                    .as_str()
                    .filter(|u| !u.is_empty())
                    .map_or_else(|| format!("{id}@eon-office-calendar"), str::to_owned);
                ids.insert(uid, id);
            } else {
                let original = e["recurrence_id"].as_str().unwrap_or_default();
                overridden.insert((parent.to_owned(), original.to_owned()));
            }
        }
        let (mut imported, mut skipped, mut known) = (0, 0, 0);
        let (series, changed): (Vec<Value>, Vec<Value>) = items
            .into_iter()
            .partition(|e| e.get("parent_uid").is_none());
        for item in series {
            let uid = item["uid"].as_str().unwrap_or_default().to_owned();
            if !uid.is_empty() && ids.contains_key(&uid) {
                known += 1;
                continue;
            }
            match self.create(&item) {
                Ok(created) => {
                    imported += 1;
                    if let (false, Some(id)) = (uid.is_empty(), created["id"].as_str()) {
                        ids.insert(uid, id.to_owned());
                    }
                }
                Err(_) => skipped += 1,
            }
        }
        for mut item in changed {
            let Some(parent) = ids.get(item["parent_uid"].as_str().unwrap_or_default()) else {
                skipped += 1;
                continue;
            };
            let key = (
                parent.clone(),
                item["recurrence_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            );
            if overridden.contains(&key) {
                known += 1;
                continue;
            }
            item["recurrence_of"] = json!(parent);
            match self.create(&item) {
                Ok(_) => {
                    imported += 1;
                    overridden.insert(key);
                }
                Err(_) => skipped += 1,
            }
        }
        Ok(json!({ "imported": imported, "known": known, "skipped": skipped }))
    }

    /// What the window should do for an `eon-office://` link: open an event
    /// (and go to its date), or come forward. Anything else is nothing.
    pub fn link(&self, text: &str) -> Option<Value> {
        let link = eon_links::parse(text)?;
        if link.app != crate::APP_ID {
            return None;
        }
        if link.item.is_empty() {
            return Some(json!({ "focus": true }));
        }
        let event = self.get(&link.item).ok()?;
        Some(json!({ "open": link.item, "date": event["start"] }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_checks_times_zones_and_rules() {
        let ok = clean(
            &json!({ "title": "Standup", "start": "2026-11-23T09:00", "tz": "Europe/Istanbul" }),
            "UTC",
        )
        .unwrap();
        assert_eq!(
            (ok["end"].clone(), ok["calendar"].clone()),
            (json!("2026-11-23T10:00"), json!("Personal"))
        );
        let day = clean(
            &json!({ "title": "Holiday", "start": "2026-12-31", "all_day": true }),
            "UTC",
        )
        .unwrap();
        assert_eq!(
            (day["start"].clone(), day["end"].clone()),
            (json!("2026-12-31"), json!("2026-12-31"))
        );
        let invited = clean(
            &json!({ "title": "Review", "start": "2026-11-23T09:00", "attendees": [
                { "name": " Ayşe ", "email": "AYSE@example.org " },
                { "name": "Ayşe again", "email": "ayse@EXAMPLE.org" },
                { "name": "No address", "email": "nobody" },
            ] }),
            "UTC",
        )
        .unwrap();
        assert_eq!(
            invited["attendees"],
            json!([{ "name": "Ayşe", "email": "AYSE@example.org" }]),
            "trimmed, one per address, no entry without an address"
        );
        for bad in [
            json!({ "title": "", "start": "2026-11-23T09:00" }),
            json!({ "title": "x", "start": "2026-11-23T09:00", "end": "2026-11-23T08:00" }),
            json!({ "title": "x", "start": "2026-11-23T09:00", "tz": "Nowhere/Land" }),
            json!({ "title": "x", "start": "2026-11-23T09:00", "rrule": "FREQ=HOURLY" }),
        ] {
            assert!(clean(&bad, "UTC").is_err(), "{bad}");
        }
    }
}
