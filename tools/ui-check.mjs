// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Drives the real EON Calendar window through the WebView2 DevTools protocol.
// Calendar is started on its own in a private EON Office home -- nothing else is
// installed or running, which is the point -- and the window is checked.
// No dependencies (Node 22+), Windows, debug build of Calendar.
//
//   node tools/ui-check.mjs --calendar <eon-calendar.exe> [--shots <dir>]

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const arg = (name) => { const at = process.argv.indexOf(name); return at > 0 ? process.argv[at + 1] : undefined; };
const appExe = arg("--calendar") && resolve(arg("--calendar"));
const shots = arg("--shots");
if (!appExe) { console.error("usage: node tools/ui-check.mjs --calendar <exe> [--shots <dir>]"); process.exit(2); }
if (shots) mkdirSync(shots, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const home = mkdtempSync(join(tmpdir(), "eon-calendar-ui-"));
const port = 9300 + Math.floor(Math.random() * 500);
let failures = 0;
const check = (name, ok, detail = "") => {
  console.log(`  ${ok ? "ok  " : "FAIL"}  ${name}${ok || !detail ? "" : `: ${detail}`}`);
  if (!ok) failures++;
};

const env = { ...process.env, EON_OFFICE_HOME: home, EON_WEBVIEW_ARGS: `--remote-debugging-port=${port}` };
// A shared preference changed elsewhere -- as the launcher would.
function setPref(key, value) {
  const r = spawnSync(appExe, ["--set-pref", key, JSON.stringify(value)], { env, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`--set-pref ${key}: ${r.stderr}`);
}

let cdp;
async function devtools() {
  for (let i = 0; i < 300; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = pages.find((p) => p.type === "page" && p.webSocketDebuggerUrl);
      if (page) {
        const ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((ok, no) => { ws.onopen = ok; ws.onerror = no; });
        let next = 1; const waiting = new Map();
        ws.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); } };
        return { send: (method, params = {}) => new Promise((ok) => { const id = next++; waiting.set(id, ok); ws.send(JSON.stringify({ id, method, params })); }), close: () => ws.close() };
      }
    } catch { /* not yet */ }
    await sleep(200);
  }
  throw new Error("the Calendar window's DevTools endpoint never appeared");
}
async function js(expression) {
  const r = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails));
  return r.result?.result?.value;
}
async function until(expression, ms = 8000) {
  const end = Date.now() + ms;
  while (Date.now() < end) { if (await js(expression).catch(() => false)) return true; await sleep(100); }
  return false;
}
async function key(keyName, code = keyName, modifiers = 0, text) {
  const vk = { ArrowDown: 40, ArrowUp: 38, Home: 36, End: 35, ArrowRight: 39, ArrowLeft: 37, Enter: 13 }[keyName] ?? keyName.toUpperCase().charCodeAt(0);
  // Shortcuts with modifiers are raw key presses, as CDP expects; plain keys may carry text.
  await cdp.send("Input.dispatchKeyEvent", { type: modifiers ? "rawKeyDown" : "keyDown", key: keyName, code, modifiers, windowsVirtualKeyCode: vk, text });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: keyName, code, modifiers, windowsVirtualKeyCode: vk });
}
// Ctrl+ shortcuts are sent as page-level keyboard events. Synthetic OS-level
// presses through CDP reach a background-started webview only intermittently,
// which tests the delivery, not Calendar; arrows still go through CDP below.
async function shortcut(letter) {
  await js(`document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: '${letter}', ctrlKey: true, bubbles: true, cancelable: true }))`);
}
async function type(text) { await cdp.send("Input.insertText", { text }); }
async function shot(name) {
  if (!shots) return;
  const r = await cdp.send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(shots, `calendar-${name}.png`), Buffer.from(r.result.data, "base64"));
}

let app;
try {
  console.log("EON Calendar interface check");
  setPref("locale", "en");
  app = spawn(appExe, [], { env, stdio: "ignore" });
  cdp = await devtools();
  await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true });
  const today = await js(`(() => { const d = new Date(); return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0'); })()`);
  const announced = (text) => `document.getElementById('announcer').textContent === ${JSON.stringify(text)}`;
  const events = (title) => `[...document.querySelectorAll('#view button.event')].filter(b => b.querySelector('.what').textContent === ${JSON.stringify(title)})`;
  const setFields = (fields) => js(`(() => { const f = ${JSON.stringify(fields)}; for (const [id, v] of Object.entries(f)) { const e = document.getElementById(id); if (e.type === 'checkbox') { e.checked = v; e.dispatchEvent(new Event('change')); } else e.value = v; } })()`);

  check("the window opens on its own, on this week", await until(`document.body.dataset.ready === '1' && document.querySelectorAll('#view .days section').length === 7`, 20000));
  check("the cursor starts on New event", await js(`document.activeElement.id === 'new'`));
  check("today is marked in the week", await js(`!!document.querySelector('#view h2[aria-current="date"]')`));
  await shot("01-week-empty");

  await js(`document.getElementById('new').click()`);
  check("New event opens the editor, title focused", await until(`!document.getElementById('editor').hidden && document.activeElement.id === 'e-title'`));
  await setFields({ "e-title": "Standup", "e-start": today, "e-start-time": "09:00", "e-end": today, "e-end-time": "09:15", "e-location": "Room 3" });
  // A rule that is not on the menu: the editor offers it as a custom choice
  // when it opens such an event; here it is added the same way.
  await js(`(() => { const s = document.getElementById('e-repeat'); s.append(new Option('custom', 'FREQ=DAILY;COUNT=5')); s.value = 'FREQ=DAILY;COUNT=5'; })()`);
  await js(`document.getElementById('editor-form').requestSubmit()`);
  check("saving a new event says so and closes the editor", (await until(announced("Added"))) && (await until(`document.getElementById('editor').hidden`)));
  check("a daily event shows in the week", await until(`${events("Standup")}.length >= 1`));

  await js(`document.querySelector('[data-mode="agenda"]').click()`);
  check("the agenda lists all five occurrences", await until(`${events("Standup")}.length === 5`));
  await shot("02-agenda");

  // One occurrence changed, another deleted: RFC 5545 exceptions from the window.
  await js(`${events("Standup")}[1].click()`);
  check("an occurrence opens in the editor", await until(`!document.getElementById('editor').hidden && document.getElementById('e-title').value === 'Standup'`));
  check("a custom rule is kept as a choice", await js(`document.getElementById('e-repeat').value === 'FREQ=DAILY;COUNT=5'`));
  await setFields({ "e-title": "Standup (late)", "e-start-time": "11:00", "e-end-time": "11:15" });
  await js(`document.getElementById('editor-form').requestSubmit()`);
  check("saving a repeating event asks: this one or all", await until(`!document.getElementById('e-ask').hidden && document.activeElement.id === 'e-one'`));
  await js(`document.getElementById('e-one').click()`);
  check("only that occurrence changes", (await until(`${events("Standup (late)")}.length === 1`)) && (await until(`${events("Standup")}.length === 4`)));
  check("it moved to its new time", await js(`${events("Standup (late)")}[0].querySelector('.when').textContent.startsWith('11:00')`));
  await js(`${events("Standup")}[2].click()`);
  await until(`!document.getElementById('editor').hidden`);
  await js(`document.getElementById('e-delete').click()`);
  check("Delete on a repeating event asks: this one or all", await until(`!document.getElementById('e-ask').hidden && !document.getElementById('e-all').hidden`));
  await js(`document.getElementById('e-one').click()`);
  check("only that occurrence goes", (await until(announced("Deleted"))) && (await until(`${events("Standup")}.length === 3`)));
  await shot("03-exceptions");

  // People: an address typed in full is invited; anything else is refused with a reason.
  await js(`document.getElementById('new').click()`);
  await until(`!document.getElementById('editor').hidden`);
  await setFields({ "e-title": "Review", "e-start": today, "e-start-time": "15:00", "e-end": today, "e-end-time": "16:00" });
  await js(`document.getElementById('e-invite').focus()`);
  await type("Ops <ops@example.org>");
  await key("Enter", "Enter", 0, "\r");
  check("a named address is invited", await until(`document.getElementById('e-people').textContent.includes('Ops <ops@example.org>')`));
  await type("nobody");
  await key("Enter", "Enter", 0, "\r");
  check("anything but an address is refused with a reason", await until(`document.getElementById('announcer').textContent.includes('is not an e-mail address')`));
  await js(`document.getElementById('e-invite').value = ''`);
  await js(`document.getElementById('editor-form').requestSubmit()`);
  await until(announced("Added"));
  const review = await js(`(async () => { const r = await window.__TAURI__.core.invoke('range', { from: '${today}', to: '${today}T23:59' }); return r.occurrences.find(o => o.title === 'Review').id; })()`);
  check("the invitation is kept with the event", await js(`(async () => (await window.__TAURI__.core.invoke('get', { id: '${review}' })).attendees[0].email === 'ops@example.org')()`));

  await js(`document.querySelector('[data-mode="month"]').click()`);
  check("the month is a table of six weeks", await until(`document.querySelectorAll('#view table.month tbody tr').length === 6`));
  await shot("04-month");
  await js(`document.querySelector('[data-mode="month"]').focus()`);
  await key("ArrowRight");
  check("arrows move between the views", await until(`document.activeElement.dataset.mode === 'agenda' && document.querySelector('[data-mode="agenda"]').getAttribute('aria-selected') === 'true'`));
  await js(`document.querySelector('[data-mode="week"]').click()`);
  await until(`document.querySelectorAll('#view .days section').length === 7`);
  const before = await js(`document.getElementById('range-title').textContent`);
  await js(`document.getElementById('next').click()`);
  check("Next moves a week on", await until(`document.getElementById('range-title').textContent !== ${JSON.stringify(before)}`));
  await js(`document.getElementById('today').click()`);
  check("Today comes back", await until(`document.getElementById('range-title').textContent === ${JSON.stringify(before)}`));

  // Another application opens an event with a link: a second start hands it
  // to this window and exits.
  const second = spawn(appExe, [`eon-office://org.eon.office.calendar/${review}`], { env, stdio: "ignore" });
  const exited = await new Promise((done) => { const timer = setTimeout(() => done(false), 15000); second.on("exit", () => { clearTimeout(timer); done(true); }); });
  check("a link opens the event in the running window", await until(`!document.getElementById('editor').hidden && document.getElementById('e-title').value === 'Review'`, 15000));
  check("the second start exits instead of opening a second window", exited);
  await js(`document.getElementById('e-close').click()`);

  // A reminder already due is shown by the window's own timer, and the event
  // keeps its version.
  await js(`(window.__fired = [], window.__TAURI__.event.listen('reminded', (e) => { window.__fired.push(...e.payload); }))`);
  const soon = await js(`(async () => { const d = new Date(Date.now() + 5 * 60000); const p = (n) => String(n).padStart(2, '0');
    const start = d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + 'T' + p(d.getHours()) + ':' + p(d.getMinutes());
    return window.__TAURI__.core.invoke('create', { fields: { title: 'Dentist', start, reminder_minutes: 10 } }); })()`);
  check("a due reminder is shown by the window's own timer", await until(`window.__fired.some(r => r.title === 'Dentist')`, 30000));
  check("showing it leaves the event's version alone", await js(`(async () => (await window.__TAURI__.core.invoke('get', { id: ${JSON.stringify(soon.id)} })).rev === ${soon.rev})()`));

  // iCalendar out and back in, through the forms a person uses.
  const ics = join(home, "out.ics");
  await js(`(() => { document.querySelector('details').open = true; document.getElementById('export-path').value = ${JSON.stringify(ics)}; document.getElementById('export-form').requestSubmit(); })()`);
  check("Export iCalendar writes the file", await until(`document.getElementById('announcer').textContent.startsWith('Saved ')`));
  await js(`(() => { document.getElementById('import-path').value = ${JSON.stringify(ics)}; document.getElementById('import-form').requestSubmit(); })()`);
  check("importing it back doubles nothing", await until(`/^Imported 0 events; [1-9][0-9]* already here/.test(document.getElementById('announcer').textContent)`), await js(`document.getElementById("announcer").textContent`));

  await shortcut("n");
  check("Ctrl+N opens a new event", await until(`!document.getElementById('editor').hidden && document.activeElement.id === 'e-title' && document.getElementById('e-title').value === ''`));
  await js(`document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))`);
  check("Escape closes the editor", await until(`document.getElementById('editor').hidden`));

  setPref("locale", "tr");
  check("a language change reaches Calendar, including built text", await until(`document.querySelector('[data-mode="week"]').textContent === 'Hafta' && document.getElementById('status').textContent.includes('etkinlik')`));
  await shot("05-turkish");

  const unnamed = await js(`[...document.querySelectorAll('button, input, select, textarea')].filter(e => {
    if (e.closest('[hidden]')) return false;
    const label = e.getAttribute('aria-label') || e.textContent.trim() || (e.id && document.querySelector('label[for="' + e.id + '"]')?.textContent.trim());
    return !label;
  }).map(e => e.outerHTML.slice(0, 80))`);
  check("every visible control has an accessible name", unnamed.length === 0, JSON.stringify(unnamed));
} catch (error) {
  failures++;
  console.error(`  FAIL  ${error.stack || error}`);
} finally {
  cdp?.close();
  try { app?.kill(); } catch { /* gone */ }
  await sleep(1500);
  try { rmSync(home, { recursive: true, force: true }); } catch { /* in use */ }
}
console.log(failures ? `\n${failures} check(s) failed.` : "\nall checks passed");
process.exit(failures ? 1 : 0);
