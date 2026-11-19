<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Contributing to EON Calendar

The organisation-wide guide,
[`.github/CONTRIBUTING.md`](https://github.com/EON-Extensible-Open-Network/.github/blob/main/CONTRIBUTING.md),
applies: DCO sign-off (`git commit -s`), Conventional Commits (`feat(calendar): ...`).
By contributing you license your work under `GPL-3.0-or-later` with the EON Module ABI
Exception (`LICENSE-EXCEPTION.md`).

- **Every string in both languages.** Add it to `src-tauri/i18n/en.json` and `tr.json`
  with the same `{placeholders}`; `--selftest` fails otherwise.
- **Every control has a name and works from the keyboard.** `checks.rs` and
  `tools/ui-check.mjs` check what a machine can; say in the pull request how you checked
  the rest.
- **A new command is registered twice:** in `generate_handler!` and in `COMMANDS`
  (`src-tauri/src/main.rs`). The interface calls it by its Rust name, `snake_case`.
- **Event text is untrusted when imported.** It reaches the page only through
  `textContent`. CI enforces it.
- **Repetition, time zones and iCalendar are standards.** A change to `rrule.rs`, `time.rs`
  or `ics.rs` comes with a test against the RFC's own example or a real producer's file —
  and a time-zone change with a test across a daylight-saving boundary.
