<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# EON Calendar

**Where to be, and when.** Events in several calendars, events that repeat — with single
occurrences moved or cancelled — time zones that hold across daylight-saving changes,
reminders that arrive as notifications, day, week, month and agenda views, and iCalendar
(`.ics`) import and export so your calendar moves to and from Outlook, Google, Apple,
Thunderbird or Nextcloud. Part of [EON Office](https://github.com/EON-Extensible-Open-Network/eon-docs/blob/main/office/roadmap.md)'s Wave 1.

> **0.1.0** — formats `v0`, early. **Stands alone:** download it, run it. Nothing else has
> to be installed or running. The window is checked end to end on Windows; Linux and macOS
> build and pass the same tests without a window. No CalDAV yet — see below.

## What it does

- **Views:** day, week (Monday first), month (six weeks, a real table a screen reader can
  move through) and agenda (the next 30 days). Previous, Today, Next; Page Up and Page Down
  step too.
- **Calendars** come from the events themselves; switch each one on or off in the sidebar.
- **Repeating events** — every day, weekday, week, two weeks, month or year from the menu,
  and the RFC 5545 rules an imported file brings (`INTERVAL`, `BYDAY` — monthly with
  ordinals such as *second Tuesday* or *last Friday* — `BYMONTHDAY`, `COUNT`, `UNTIL`). A
  rule this calendar does not run (`BYSETPOS`, `BYMONTH`, …) keeps the first occurrence and
  says so in the notes, rather than repeating it wrongly. **One occurrence** can be changed or deleted on its
  own: saving or deleting asks *only this one* or *every occurrence*.
- **Time zones** from the IANA database. An event is kept as it was meant — 09:00 in
  `Europe/Istanbul` — so a weekly meeting stays at 09:00 there whatever daylight saving does
  elsewhere, and is shown at the right time in this computer's zone. All-day events are the
  same days everywhere.
- **Reminders** appear in your operating system's notification centre, from *at the start*
  to *a week before*. Each occurrence fires once; one that fell due while Calendar was closed
  is shown when you next open it, if the event has not ended.
- **People:** invite by e-mail address; with EON Contacts installed, by name.
- **iCalendar** export and import: Outlook's Windows time-zone names, Google's UTC times and
  `DURATION`, Apple's all-day dates, `EXDATE`, `RECURRENCE-ID`, alarms, attendees. Importing
  a file twice — or Calendar's own export — doubles nothing.
- **Two windows never overwrite each other:** a change made on an out-of-date version is
  refused, and a reminder firing is not a change.
- **Keyboard:** Ctrl+N opens a new event; arrows move between the views; Escape closes the
  editor. Follows the language, theme, text size, contrast and motion preferences shared by
  EON Office applications, live.
- **One window.** Starting Calendar again, or following a link to an event, brings the open
  window forward.

**With other EON Office applications** (all optional): `eon-office://org.eon.office.calendar/<id>`
opens an event from any application that links to it. Calendar adds what you create and edit
to the shared *Recent* list — titles only. It reads the names and addresses EON Contacts
publishes, and nothing else of Contacts.

**What it asks for:** keep its data on this device (`storage`), make its events searchable
(`search.index`, private to Calendar), search (`search.query`), show reminders
(`notifications`, `schedule`), add titles to your recent list (`events.publish`), read the
names and addresses EON Contacts publishes (`call:org.eon.office.contacts`).

**Not yet:** CalDAV (Nextcloud, Radicale). It needs a password kept somewhere better than the
calendar's own store; that storage is designed once, together with EON Mail and Contacts'
CardDAV, and CalDAV follows it.

## Install

Download `eon-calendar` from a release and run it — see the release notes for checksums. Or,
with EON Launcher: *Get applications* → *Include previews* → EON Calendar.

## Develop

```
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
node tools/ui-check.mjs --calendar target/debug/eon-calendar.exe --shots shots
```

Licence `GPL-3.0-or-later` with the [EON Module ABI Exception](LICENSE-EXCEPTION.md).
Contributing: [CONTRIBUTING.md](CONTRIBUTING.md). Security: [SECURITY.md](SECURITY.md).
