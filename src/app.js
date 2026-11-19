// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// EON Calendar's interface. No network of its own; text from events (which
// may be imported from anywhere) enters the page only through textContent.
//
// Dates are `YYYY-MM-DD` strings and times `YYYY-MM-DDTHH:MM` strings, as the
// host speaks them. Arithmetic on them goes through UTC so that a daylight-
// saving change on this computer can never shift a day; zones are the host's
// business.

"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

let strings = {};
let mode = "week";
let cursor = ""; // the day the view is on; set at start
let zone = "UTC";
let occurrences = [];
let calendars = {};
const hidden = new Set(); // calendars switched off in the sidebar
let editing = null; // { event, occurrence, opened } while the editor is open
let invited = []; // the editor's people
let ask = null; // "save" | "delete" while the scope question is up

function t(key, vars = {}) {
  let text = strings[key] ?? key;
  for (const [n, v] of Object.entries(vars)) text = text.replaceAll(`{${n}}`, String(v));
  return text;
}
const reason = (e) => (e && (e.message || e.code)) || String(e);
const failed = (e) => announce(e && e.code === "conflict" ? t("conflict") : t("error", { reason: reason(e) }));

// ---- dates -------------------------------------------------------------------

const pad = (n) => String(n).padStart(2, "0");
const ymd = (d) => `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
function utc(text) {
  const [y, m, d] = text.slice(0, 10).split("-").map(Number);
  const [h, min] = text.length > 10 ? text.slice(11, 16).split(":").map(Number) : [0, 0];
  return new Date(Date.UTC(y, m - 1, d, h, min));
}
function todayYmd() {
  const d = new Date();
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}
function addDays(day, n) {
  const d = utc(day);
  d.setUTCDate(d.getUTCDate() + n);
  return ymd(d);
}
function addMinutes(wall, n) {
  const d = new Date(utc(wall).getTime() + n * 60000);
  return `${ymd(d)}T${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
}
const minutesBetween = (a, b) => Math.round((utc(b) - utc(a)) / 60000);
const weekday = (day) => (utc(day).getUTCDay() + 6) % 7; // Monday is 0

function fmt(day, options) {
  return new Intl.DateTimeFormat(document.documentElement.lang, { ...options, timeZone: "UTC" }).format(utc(day));
}
const longDate = (day) => fmt(day, { weekday: "long", day: "numeric", month: "long", year: "numeric" });
const shortDate = (day) => fmt(day, { weekday: "short", day: "numeric", month: "short" });

/// Does occurrence `o` show on `day`? An event ending at midnight does not
/// spill into the next day.
function onDay(o, day) {
  const s = o.start.slice(0, 10);
  const e = o.end.slice(0, 10);
  if (o.all_day) return s <= day && day <= e;
  if (s > day || e < day) return false;
  return e > day || s === day || o.end.slice(11) !== "00:00";
}

// ---- preferences and strings -------------------------------------------------

async function loadStrings(locale) {
  const language = locale && locale !== "system"
    ? (locale.startsWith("tr") ? "tr" : "en")
    : ((navigator.language || "en").toLowerCase().startsWith("tr") ? "tr" : "en");
  strings = await invoke("strings", { language });
  document.documentElement.lang = language;
  for (const n of document.querySelectorAll("[data-i18n]")) n.textContent = t(n.dataset.i18n);
  for (const n of document.querySelectorAll("[data-i18n-label]")) n.setAttribute("aria-label", t(n.dataset.i18nLabel));
  for (const n of document.querySelectorAll("[data-i18n-placeholder]")) n.setAttribute("placeholder", t(n.dataset.i18nPlaceholder));
}

async function applyPrefs() {
  const p = await invoke("prefs");
  const root = document.documentElement;
  if (p.theme === "light" || p.theme === "dark") root.dataset.theme = p.theme; else delete root.dataset.theme;
  if (p.high_contrast) root.dataset.contrast = "high"; else delete root.dataset.contrast;
  if (p.reduce_motion) root.dataset.motion = "reduce"; else delete root.dataset.motion;
  root.style.setProperty("--scale", String(p.font_scale || 1));
  await loadStrings(p.locale);
}

function announce(text) {
  $("announcer").textContent = "";
  window.setTimeout(() => { $("announcer").textContent = text; }, 30);
}

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v);
  }
  for (const c of children) if (c !== null && c !== undefined && c !== false) node.append(c);
  return node;
}

// ---- the period on screen ------------------------------------------------------

function period() {
  if (mode === "day") return [cursor, addDays(cursor, 1)];
  if (mode === "week") {
    const s = addDays(cursor, -weekday(cursor));
    return [s, addDays(s, 7)];
  }
  if (mode === "month") {
    const first = `${cursor.slice(0, 8)}01`;
    const s = addDays(first, -weekday(first));
    return [s, addDays(s, 42)];
  }
  return [cursor, addDays(cursor, 30)];
}

function step(n) {
  if (mode === "month") {
    const d = utc(`${cursor.slice(0, 8)}01`);
    d.setUTCMonth(d.getUTCMonth() + n);
    cursor = ymd(d);
  } else {
    cursor = addDays(cursor, n * ({ day: 1, week: 7, agenda: 30 })[mode]);
  }
  refresh();
}

function periodTitle([from, to]) {
  if (mode === "day") return longDate(cursor);
  if (mode === "month") return fmt(cursor, { month: "long", year: "numeric" });
  if (mode === "agenda") return t("agenda.from", { date: longDate(from) });
  const last = addDays(to, -1);
  return new Intl.DateTimeFormat(document.documentElement.lang, { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" })
    .formatRange(utc(from), utc(last));
}

async function refresh() {
  const [from, to] = period();
  try {
    const r = await invoke("range", { from, to });
    occurrences = r.occurrences;
    calendars = r.calendars;
  } catch (e) {
    failed(e);
    return;
  }
  $("range-title").textContent = periodTitle([from, to]);
  renderCalendars();
  const shown = occurrences.filter((o) => !hidden.has(o.calendar || "Personal"));
  $("status").textContent = `${shown.length ? t("count", { count: shown.length }) : t("empty")} · ${t("zone.note", { zone })}`;
  const days = [];
  for (let d = from; d < to; d = addDays(d, 1)) days.push(d);
  const view = mode === "month" ? renderMonth(days, shown)
    : mode === "agenda" ? renderAgenda(days, shown)
      : renderDays(days, shown);
  $("view").replaceChildren(view);
}

function renderCalendars() {
  const names = Object.keys(calendars);
  if (!names.length) names.push("Personal");
  $("calendars").replaceChildren(...names.map((name, i) => {
    const box = el("input", { type: "checkbox", id: `cal-${i}` });
    box.checked = !hidden.has(name);
    box.addEventListener("change", () => {
      if (box.checked) hidden.delete(name); else hidden.add(name);
      refresh();
    });
    return el("li", {}, box, el("label", { for: `cal-${i}`, text: name }));
  }));
  $("calendar-names").replaceChildren(...names.map((n) => el("option", { value: n })));
}

// ---- views ---------------------------------------------------------------------

function timeText(o, day) {
  if (o.all_day) return t("allday");
  const from = o.start.slice(0, 10) === day ? o.start.slice(11, 16) : "…";
  const to = o.end.slice(0, 10) === day ? o.end.slice(11, 16) : "…";
  return `${from}–${to}`;
}

function eventButton(o, day, withTime = true) {
  const when = timeText(o, day);
  const label = [o.title, shortDate(day), when, o.location, o.repeats ? t("event.repeats") : ""].filter(Boolean).join(", ");
  return el("button", {
    type: "button", class: `event${o.all_day ? " allday" : ""}`, "aria-label": label,
    "data-id": o.id, "data-occurrence": o.occurrence, onclick: () => openEvent(o),
  },
  withTime && !o.all_day ? el("span", { class: "when", text: when }) : null,
  el("span", { class: "what", text: o.title }),
  o.repeats ? el("span", { class: "repeat", "aria-hidden": "true", text: "↻" }) : null);
}

function dayHeading(day, level = "h2") {
  return el("div", { class: "day-head" },
    el(level, { text: shortDate(day), "aria-current": day === todayYmd() ? "date" : "false" }),
    el("button", { type: "button", class: "secondary add", text: "+", "aria-label": t("new.on", { date: longDate(day) }), onclick: () => newEvent(day) }));
}

function renderDays(days, shown) {
  return el("div", { class: `days days-${days.length}` }, ...days.map((day) => {
    const here = shown.filter((o) => onDay(o, day));
    return el("section", { class: day === todayYmd() ? "today" : "", "aria-label": longDate(day) },
      dayHeading(day),
      el("ul", { class: "events" }, ...here.map((o) => el("li", {}, eventButton(o, day)))));
  }));
}

function renderMonth(days, shown) {
  const month = cursor.slice(0, 7);
  const head = el("tr", {}, ...days.slice(0, 7).map((d) => el("th", { scope: "col", abbr: fmt(d, { weekday: "long" }), text: fmt(d, { weekday: "short" }) })));
  const rows = [];
  for (let w = 0; w < 6; w += 1) {
    rows.push(el("tr", {}, ...days.slice(w * 7, w * 7 + 7).map((day) => {
      const here = shown.filter((o) => onDay(o, day));
      const extra = here.length - 3;
      return el("td", { class: `${day.slice(0, 7) === month ? "" : "other"}${day === todayYmd() ? " today" : ""}` },
        el("button", {
          type: "button", class: "daynum", text: String(Number(day.slice(8))),
          "aria-label": t("goto.day", { date: longDate(day) }), "aria-current": day === todayYmd() ? "date" : "false",
          onclick: () => { cursor = day; setMode("day"); },
        }),
        el("ul", { class: "events" }, ...here.slice(0, 3).map((o) => el("li", {}, eventButton(o, day, false)))),
        extra > 0 ? el("button", { type: "button", class: "more secondary", text: t("more", { count: extra }), "aria-label": t("goto.day", { date: longDate(day) }), onclick: () => { cursor = day; setMode("day"); } }) : null);
    })));
  }
  return el("table", { class: "month" },
    el("caption", { class: "sr-only", text: fmt(cursor, { month: "long", year: "numeric" }) }),
    el("thead", {}, head), el("tbody", {}, ...rows));
}

function renderAgenda(days, shown) {
  const parts = days
    .map((day) => [day, shown.filter((o) => onDay(o, day))])
    .filter(([, here]) => here.length)
    .map(([day, here]) => el("section", { "aria-label": longDate(day) },
      el("h2", { text: longDate(day), "aria-current": day === todayYmd() ? "date" : "false" }),
      el("ul", { class: "events agenda" }, ...here.map((o) => el("li", {}, eventButton(o, day),
        o.location ? el("span", { class: "muted", text: o.location }) : null)))));
  return el("div", {}, ...(parts.length ? parts : [el("p", { class: "muted", text: t("empty") })]));
}

function setMode(next) {
  mode = next;
  for (const tab of document.querySelectorAll("#modes [role=tab]")) {
    const on = tab.dataset.mode === next;
    tab.setAttribute("aria-selected", String(on));
    tab.tabIndex = on ? 0 : -1;
  }
  refresh();
}
const MODES = ["day", "week", "month", "agenda"];
for (const tab of document.querySelectorAll("#modes [role=tab]")) {
  tab.addEventListener("click", () => setMode(tab.dataset.mode));
  tab.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const next = MODES[(MODES.indexOf(mode) + (e.key === "ArrowRight" ? 1 : 3)) % 4];
    setMode(next);
    document.querySelector(`#modes [data-mode=${next}]`).focus();
  });
}
$("prev").addEventListener("click", () => step(-1));
$("next").addEventListener("click", () => step(1));
$("today").addEventListener("click", () => { cursor = todayYmd(); refresh(); });

// ---- the editor ----------------------------------------------------------------

function setTimed() {
  const timed = !$("e-allday").checked;
  for (const n of document.querySelectorAll("#editor .time")) n.hidden = !timed;
}
$("e-allday").addEventListener("change", setTimed);

function choose(select, value, custom) {
  const known = [...select.options].some((o) => o.value === value);
  select.querySelector("option[data-custom]")?.remove();
  if (!known) select.append(el("option", { value, "data-custom": "1", text: custom }));
  select.value = value;
}

function renderPeople() {
  $("e-people").replaceChildren(...invited.map((p, i) => el("li", {},
    el("span", { text: p.name ? `${p.name} <${p.email}>` : p.email }),
    el("button", {
      type: "button", class: "secondary", text: "×", "aria-label": t("remove", { name: p.name || p.email }),
      onclick: () => { invited.splice(i, 1); renderPeople(); $("e-invite").focus(); },
    }))));
}

function fill(fields) {
  $("e-title").value = fields.title || "";
  $("e-allday").checked = Boolean(fields.all_day);
  $("e-start").value = fields.start.slice(0, 10);
  $("e-start-time").value = fields.all_day ? "09:00" : fields.start.slice(11, 16);
  $("e-end").value = fields.end.slice(0, 10);
  $("e-end-time").value = fields.all_day ? "10:00" : fields.end.slice(11, 16);
  $("e-zone").value = fields.tz || zone;
  choose($("e-repeat"), fields.rrule || "", t("repeat.custom", { rule: fields.rrule }));
  const r = fields.reminder_minutes;
  choose($("e-reminder"), r === null || r === undefined ? "" : String(r), t("reminder.custom", { count: r }));
  $("e-calendar").value = fields.calendar || "Personal";
  $("e-location").value = fields.location || "";
  $("e-notes").value = fields.notes || "";
  invited = (fields.attendees || []).map((p) => ({ name: p.name || "", email: p.email }));
  renderPeople();
  setTimed();
  $("e-ask").hidden = true;
  $("e-delete").hidden = !editing.event;
  $("layout").classList.add("with-editor");
  $("editor").hidden = false;
  $("e-title").focus();
}

function newEvent(day = cursor) {
  const now = new Date();
  const hour = day === todayYmd() ? Math.min(now.getHours() + 1, 23) : 9;
  const start = `${day}T${pad(hour)}:00`;
  const first = Object.keys(calendars).find((c) => !hidden.has(c));
  editing = { event: null, occurrence: null };
  fill({ start, end: addMinutes(start, 60), tz: zone, calendar: first || "Personal", reminder_minutes: null });
}
$("new").addEventListener("click", () => newEvent());

async function openEvent(o) {
  let event;
  try {
    event = await invoke("get", { id: o.id });
  } catch (e) {
    failed(e);
    return;
  }
  // One occurrence of a series: the form shows that occurrence, in the
  // event's own zone, and saving asks whether it is this one or all of them.
  const series = Boolean(event.rrule) && !event.recurrence_of;
  editing = { event, occurrence: series ? o.occurrence : null, opened: o };
  if (!series) {
    fill(event);
    return;
  }
  const length = event.all_day
    ? minutesBetween(event.start, event.end) / 1440
    : minutesBetween(event.start, event.end);
  const end = event.all_day ? addDays(o.occurrence, length) : addMinutes(o.occurrence, length);
  fill({ ...event, start: o.occurrence, end });
}

function closeEditor() {
  const was = editing?.opened;
  editing = null;
  ask = null;
  $("editor").hidden = true;
  $("layout").classList.remove("with-editor");
  const back = was && document.querySelector(`#view [data-id="${was.id}"][data-occurrence="${was.occurrence}"]`);
  (back || $("new")).focus();
}
$("e-close").addEventListener("click", closeEditor);
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && editing) closeEditor(); });

function collect() {
  const allDay = $("e-allday").checked;
  const start = allDay ? $("e-start").value : `${$("e-start").value}T${$("e-start-time").value || "00:00"}`;
  let end = allDay ? $("e-end").value : `${$("e-end").value || $("e-start").value}T${$("e-end-time").value || "00:00"}`;
  if (!$("e-end").value) end = allDay ? start : addMinutes(start, 60);
  const reminder = $("e-reminder").value;
  return {
    title: $("e-title").value.trim(), all_day: allDay, start, end,
    tz: $("e-zone").value.trim() || zone, rrule: $("e-repeat").value,
    reminder_minutes: reminder === "" ? null : Number(reminder),
    calendar: $("e-calendar").value.trim(), location: $("e-location").value.trim(),
    notes: $("e-notes").value, attendees: invited,
  };
}

/// The series moved by what was done to this occurrence: the same shift, the
/// new length.
function forSeries(fields) {
  const { event, occurrence } = editing;
  if (fields.all_day !== Boolean(event.all_day)) {
    const date = event.start.slice(0, 10);
    const start = fields.all_day ? date : `${date}T${fields.start.slice(11, 16)}`;
    const end = fields.all_day ? addDays(date, Math.max(0, minutesBetween(fields.start, fields.end) / 1440))
      : addMinutes(start, minutesBetween(fields.start, fields.end));
    return { ...fields, start, end };
  }
  const shift = minutesBetween(occurrence, fields.start);
  const length = minutesBetween(fields.start, fields.end);
  if (fields.all_day) {
    const start = addDays(event.start, Math.round(shift / 1440));
    return { ...fields, start, end: addDays(start, Math.round(length / 1440)) };
  }
  const start = addMinutes(event.start, shift);
  return { ...fields, start, end: addMinutes(start, length) };
}

async function save(scope) {
  const fields = collect();
  try {
    if (!editing.event) {
      await invoke("create", { fields });
      announce(t("created"));
      cursor = fields.start.slice(0, 10);
    } else if (scope === "one") {
      const { rrule, ...changes } = fields;
      await invoke("change_occurrence", { id: editing.event.id, occurrence: editing.occurrence, changes });
      announce(t("saved"));
    } else {
      const next = scope === "all" ? forSeries(fields) : fields;
      const r = await invoke("update", { id: editing.event.id, fields: next, rev: editing.event.rev });
      editing.event.rev = r.rev;
      announce(t("saved"));
    }
  } catch (e) {
    failed(e);
    return;
  }
  closeEditor();
  await refresh();
}

async function remove(scope) {
  const { event, occurrence } = editing;
  try {
    if (scope === "one") await invoke("delete_occurrence", { id: event.id, occurrence, rev: event.rev });
    else await invoke("delete", { id: event.id });
    announce(t("deleted"));
  } catch (e) {
    failed(e);
    return;
  }
  closeEditor();
  await refresh();
}

function question(kind) {
  ask = kind;
  const series = Boolean(editing.occurrence);
  $("e-question").textContent = series
    ? t(kind === "save" ? "scope.save" : "scope.delete")
    : t("delete.confirm", { title: editing.event.title });
  $("e-one").textContent = series ? t("scope.one") : t("delete");
  $("e-all").hidden = !series;
  $("e-ask").hidden = false;
  $("e-one").focus();
}

$("editor-form").addEventListener("submit", (event) => {
  event.preventDefault();
  if (editing.occurrence) question("save"); else save(null);
});
$("e-delete").addEventListener("click", () => question("delete"));
$("e-one").addEventListener("click", () => {
  const series = Boolean(editing.occurrence);
  if (ask === "save") save("one");
  else remove(series ? "one" : "all");
});
$("e-all").addEventListener("click", () => (ask === "save" ? save("all") : remove("all")));
$("e-cancel").addEventListener("click", () => { $("e-ask").hidden = true; ask = null; $("e-title").focus(); });

// People: names come from EON Contacts when it is installed; an address
// typed in full always works.
let lookup = 0;
$("e-invite").addEventListener("input", () => {
  window.clearTimeout(lookup);
  const query = $("e-invite").value.trim();
  lookup = window.setTimeout(async () => {
    try {
      const r = await invoke("people", { query });
      $("people-found").replaceChildren(...r.matches.map((p) => el("option", { value: `${p.name} <${p.email}>` })));
    } catch {
      $("people-found").replaceChildren();
    }
  }, 150);
});
$("e-invite").addEventListener("keydown", (e) => {
  if (e.key !== "Enter") return;
  e.preventDefault();
  const text = $("e-invite").value.trim();
  if (!text) return;
  const named = text.match(/^(.*)<([^<>\s]+@[^<>\s]+)>$/);
  const person = named ? { name: named[1].trim(), email: named[2] }
    : /^[^\s@]+@[^\s@]+$/.test(text) ? { name: "", email: text } : null;
  if (!person) {
    announce(t("invite.bad", { text }));
    return;
  }
  if (!invited.some((p) => p.email.toLowerCase() === person.email.toLowerCase())) invited.push(person);
  $("e-invite").value = "";
  renderPeople();
  announce(t("invite.added", { name: person.name || person.email }));
});

// ---- iCalendar ------------------------------------------------------------------

$("export-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const r = await invoke("export_ics", { path: $("export-path").value });
    announce(t("exported", { count: r.count, path: r.path }));
    $("status").textContent = t("exported", { count: r.count, path: r.path });
  } catch (e) {
    failed(e);
  }
});
$("import-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const r = await invoke("import_ics", { path: $("import-path").value });
    await refresh();
    announce(t("import.done", { count: r.imported, known: r.known, skipped: r.skipped }));
    $("status").textContent = t("import.done", { count: r.imported, known: r.known, skipped: r.skipped });
  } catch (e) {
    failed(e);
  }
});

// ---- start ---------------------------------------------------------------------

async function show(payload) {
  if (!payload) return;
  if (payload.date) cursor = payload.date.slice(0, 10);
  await refresh();
  if (payload.open) {
    const o = occurrences.find((x) => x.id === payload.open) || { id: payload.open, occurrence: payload.date };
    await openEvent(o);
  }
}
listen("show", ({ payload }) => show(payload));
listen("prefs-changed", async () => { await applyPrefs(); await refresh(); });
document.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") { e.preventDefault(); newEvent(); return; }
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "");
  if (typing || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key === "PageUp") { e.preventDefault(); step(-1); }
  if (e.key === "PageDown") { e.preventDefault(); step(1); }
});

async function start() {
  await loadStrings("system");
  cursor = cursor || todayYmd();
  try {
    const s = await invoke("status");
    zone = s.zone;
    $("zones").replaceChildren(...(Intl.supportedValuesOf ? Intl.supportedValuesOf("timeZone") : [zone]).map((z) => el("option", { value: z })));
    await applyPrefs();
    $("banner").hidden = true;
    $("layout").hidden = false;
    await refresh();
    if (document.activeElement === document.body || document.activeElement === null) $("new").focus();
    await show(await invoke("initial"));
    document.body.dataset.ready = "1";
  } catch (e) {
    $("layout").hidden = true;
    $("banner").hidden = false;
    $("banner-text").textContent = t("open.failed", { reason: reason(e) });
  }
}
$("retry").addEventListener("click", start);
start();
