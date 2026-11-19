<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Changelog

## 0.1.0 — first version

- **Stands alone** on the `eon-office-core` library, in the EON Office home folder (plan
  madde 43; `eon-docs/office/design/2026-11-19-library-core.md`).
- Day, week, month and agenda views; calendars switched on and off in the sidebar.
- Events with RFC 5545 repetition (`DAILY` … `YEARLY`, `INTERVAL`, `BYDAY` — monthly with
  ordinals such as `2TU` or `-1FR` — `BYMONTHDAY`, `COUNT`, `UNTIL`; anything else is
  refused by name); a single occurrence changed (an
  event of its own, `RECURRENCE-ID`) or deleted (`EXDATE`) from the window, which asks
  *only this one* or *every occurrence*. Deleting a changed occurrence deletes that
  occurrence; the original does not come back.
- Time zones from the IANA database: events keep their wall-clock time in their own zone
  and are shown in this computer's. `--selftest` fails if the build has no zone database.
- Reminders as notifications, from the window's own timer: each occurrence once, a missed
  one at the next start if the event has not ended. The shown-mark lives beside the event,
  so a reminder never makes an open editor lose its save.
- People invited by address, or by name through what EON Contacts publishes.
- iCalendar import and export: Outlook (Windows zone names), Google (UTC, `DURATION`),
  Apple (all-day dates), alarms, exceptions, attendees. An event already here — same UID,
  or Calendar's own export — is counted and left alone.
- One window: a second start, or an `eon-office://org.eon.office.calendar/<id>` link, goes
  to the running window.
- `--selftest` checks the strings, the interface, that every command the page invokes is
  registered, the manifest and the zone database; `tools/ui-check.mjs` drives the real
  window.
- Not in this version: CalDAV, which waits for the keychain credential storage (roadmap,
  changed 19 November 2026).
