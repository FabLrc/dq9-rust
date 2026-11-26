// Drives one Web pilot session in a real Chromium (Edge or Chrome) over the DevTools protocol:
// serves the wasm bundle, opens the page with the session's query string, picks the ROM with
// the page's <input type=file> (the file never leaves the machine: the server only serves the
// bundle), then follows window.dq9_session (app/auto/session.rs) until the run stops.
//
// Needs Node 22+ (global WebSocket and fetch) and no npm package. Run through
// scripts/web-session.ps1, which builds the bundle first.
//
//   node web/session-driver.mjs --site build/web-session/site --rom path.nds \
//        --out build/evidence/j4-web-pilot/run1 --query "newgame=1&pilot=1&route=2750&seed=7&mute=1"
//   node web/session-driver.mjs --url https://fablrc.github.io/dq9-rust/ --rom path.nds \
//        --out build/evidence/j4-web-show/hosted --query "newgame=1&pilot=1&battles=show"
//
// `--mode record` (with `?record=1`) plays a short session by hand through the page — real key
// events on the canvas — and saves the record with the page's "Save recording" button, keeping
// the download in `--out/downloads`; `--mode replay --record <file>` (with `?play-record=1`)
// picks the `.dq9rec` and the ROM in the page and reads the stop line
// (`docs/app.md`, "Recording a human session").
//
// `--url` drives an already served page (the hosted Pages deployment) instead of `--site`: no
// local server is started, `--query` becomes that page's query string, and the ROM is still
// chosen through the page's <input type=file>.
//
// Writes into --out: console.log (every console line, with the wall time), progress.log,
// checkpoints.jsonl (the native --checkpoints format), report.txt (the journal and the final
// state), shots/*.png (the milestones), result.json.

import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const arg = process.argv[i];
  if (!arg.startsWith("--")) continue;
  const eq = arg.indexOf("=");
  if (eq > 0) args[arg.slice(2, eq)] = arg.slice(eq + 1);
  else args[arg.slice(2)] = process.argv[i + 1]?.startsWith("--") ? "1" : (process.argv[++i] ?? "1");
}
const need = (key) => {
  if (!args[key]) throw new Error(`missing --${key}`);
  return args[key];
};
const rom = path.resolve(need("rom"));
const out = path.resolve(need("out"));
const query = args.query ?? "newgame=1&pilot=1&fight=1&route=2750&seed=7&mute=1";
// Either the caller's page (--url, the hosted deployment) or a bundle served from --site.
const site = args.site ? path.resolve(args.site) : null;
const hosted = args.url ? new URL(args.url) : null;
if (!site && !hosted) throw new Error("missing --site or --url");
if (hosted) hosted.search = query;
const port = Number(args.port ?? 8765);
const debugPort = Number(args["debug-port"] ?? 9333);
const maxWall = Number(args["max-wall"] ?? 4 * 3600) * 1000;
const stallLimit = Number(args.stall ?? 600) * 1000;
const browserPath = args.browser ?? [
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
].find((p) => fs.existsSync(p));
if (!browserPath) throw new Error("no Chromium-based browser found (--browser)");
const extra = (args["browser-args"] ?? "").split(" ").filter(Boolean);

fs.mkdirSync(path.join(out, "shots"), { recursive: true });
const consoleLog = fs.createWriteStream(path.join(out, "console.log"));
const progressLog = fs.createWriteStream(path.join(out, "progress.log"));
const checkpointFile = fs.createWriteStream(path.join(out, "checkpoints.jsonl"));
const started = Date.now();
const wall = () => ((Date.now() - started) / 1000).toFixed(1).padStart(8);
const say = (text) => {
  console.log(`[${wall()}] ${text}`);
  progressLog.write(`[${wall()}] ${text}\n`);
};

// ---- static server (the bundle only; none with --url) ----
let server = null;
if (site) {
  const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm" };
  server = http.createServer((req, res) => {
    const name = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = path.join(site, name === "/" ? "index.html" : name);
    if (!file.startsWith(site) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
    fs.createReadStream(file).pipe(res);
  });
  await new Promise((resolve) => server.listen(port, "127.0.0.1", resolve));
}

// ---- browser ----
// The browser profile is scratch: outside the evidence folder, one per DevTools port.
const profile = path.join(os.tmpdir(), `dq9-web-session-${debugPort}`);
// A previous browser of the same port may still be closing: the profile is scratch, so a
// locked file must not abort the run.
try {
  fs.rmSync(profile, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
} catch (e) {
  say(`profile left in place (${e.code}); the browser reuses it`);
}
const browser = spawn(
  browserPath,
  [
    `--remote-debugging-port=${debugPort}`,
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-timer-throttling",
    "--disable-renderer-backgrounding",
    "--disable-backgrounding-occluded-windows",
    "--window-size=1280,900",
    ...extra,
    "about:blank",
  ],
  { stdio: "ignore" },
);
const stopBrowser = () => {
  try {
    browser.kill();
  } catch {}
};
process.on("exit", stopBrowser);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
let version;
for (let i = 0; i < 100 && !version; i++) {
  try {
    version = await (await fetch(`http://127.0.0.1:${debugPort}/json/version`)).json();
  } catch {
    await sleep(200);
  }
}
if (!version) throw new Error("the browser did not open its DevTools port");
say(`browser ${version.Browser}`);
const target = await (await fetch(`http://127.0.0.1:${debugPort}/json/new?about:blank`, { method: "PUT" })).json();

const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});
let nextId = 1;
const pending = new Map();
const listeners = [];
socket.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(`${message.error.message}`));
    else resolve(message.result);
  } else if (message.method) {
    for (const listener of listeners) listener(message);
  }
};
const cdp = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expression) => {
  const result = await cdp("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.text + " " + (result.exceptionDetails.exception?.description ?? ""));
  return result.result.value;
};

// ---- console, errors, crashes ----
let consoleErrors = 0;
let crashed = false;
const consoleHooks = [];
listeners.push((message) => {
  if (message.method === "Runtime.consoleAPICalled") {
    const { type, args: values } = message.params;
    const text = values.map((v) => v.value ?? v.description ?? "").join(" ");
    if (type === "error") consoleErrors++;
    // The final report is one multi-line console entry; keep it whole in console.log.
    consoleLog.write(`[${wall()}] ${type}: ${text}\n`);
    for (const hook of consoleHooks) hook(type, text);
  } else if (message.method === "Runtime.exceptionThrown") {
    consoleErrors++;
    const details = message.params.exceptionDetails;
    consoleLog.write(`[${wall()}] exception: ${details.text} ${details.exception?.description ?? ""}\n`);
  } else if (message.method === "Log.entryAdded") {
    const { level, text, url } = message.params.entry;
    if (level === "error") consoleErrors++;
    consoleLog.write(`[${wall()}] log.${level}: ${text} ${url ?? ""}\n`);
  } else if (message.method === "Inspector.targetCrashed") {
    crashed = true;
    say("the page crashed (Inspector.targetCrashed)");
  }
});
await cdp("Runtime.enable");
await cdp("Log.enable");
await cdp("Page.enable");
await cdp("DOM.enable");
await cdp("Emulation.setDeviceMetricsOverride", { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });

// ---- page modes: record and replay (`docs/app.md`, § Recording a human session) ----
// `--mode record` plays a short session by hand through the page (`?record=1`), presses real
// keys, saves the record with the page's "Save recording" button and keeps the download;
// `--mode replay` picks a `.dq9rec` in the page (`?play-record=1`) with the ROM and reads the
// stop line. The pilot session is the default mode (below).
const mode = args.mode ?? "pilot";
if (mode === "record" || mode === "replay") {
  const pageText = (selector) =>
    evaluate(
      `(() => { const el = document.querySelector(${JSON.stringify(selector)});
        return el ? el.textContent : null; })()`,
    ).catch(() => null);
  const stepsShown = async () => {
    const match = /(\d+) steps/.exec((await pageText("#recording")) ?? "");
    return match ? Number(match[1]) : 0;
  };
  const waitForRom = async () => {
    for (let i = 0; i < 1200; i++) {
      const ready = await evaluate(
        "(() => { const r = document.querySelector('#rom'); return !!r && !r.disabled; })()",
      ).catch(() => false);
      if (ready) return true;
      await sleep(250);
    }
    return false;
  };
  const pickFile = async (selector, file) => {
    const doc = await cdp("DOM.getDocument", { depth: 0 });
    const node = await cdp("DOM.querySelector", { nodeId: doc.root.nodeId, selector });
    if (!node.nodeId) throw new Error(`no ${selector} field in the page`);
    await cdp("DOM.setFileInputFiles", { files: [file], nodeId: node.nodeId });
  };
  // Real keyboard events on the canvas: the same `App::key` path a person drives.
  const virtualKeys = {
    Enter: 13, Space: 32, ArrowUp: 38, ArrowDown: 40, ArrowLeft: 37, ArrowRight: 39,
    KeyW: 87, KeyA: 65, KeyS: 83, KeyD: 68, KeyZ: 90, KeyX: 88, KeyI: 73, Escape: 27,
  };
  const keyText = (name) =>
    name === "Space" ? " " : name.startsWith("Key") ? name.slice(3).toLowerCase() : name;
  const keyEvent = (type, name) =>
    cdp("Input.dispatchKeyEvent", {
      type,
      key: keyText(name),
      code: name,
      windowsVirtualKeyCode: virtualKeys[name] ?? 0,
      nativeVirtualKeyCode: virtualKeys[name] ?? 0,
    });
  const tap = async (name = "Enter", gap = 220) => {
    await keyEvent("keyDown", name);
    await sleep(60);
    await keyEvent("keyUp", name);
    await sleep(gap);
  };
  const hold = async (name, ms) => {
    await keyEvent("keyDown", name);
    await sleep(ms);
    await keyEvent("keyUp", name);
    await sleep(150);
  };
  const shot = async (name) => {
    const file = `shots/${name}.png`;
    try {
      const png = await cdp("Page.captureScreenshot", { format: "png" });
      fs.writeFileSync(path.join(out, file), Buffer.from(png.data, "base64"));
      say(`screenshot ${file}`);
    } catch (e) {
      say(`screenshot ${file} failed: ${e.message}`);
    }
  };

  const url = hosted ? hosted.toString() : `http://127.0.0.1:${port}/?${query}`;
  say(`open ${url}`);
  await cdp("Page.navigate", { url });
  if (!(await waitForRom())) throw new Error("the page never offered the ROM field");
  let recordFile = null;
  let stop = null;
  let ok = false;

  if (mode === "record") {
    const target = Number(args.steps ?? 3000);
    const downloads = path.join(out, "downloads");
    fs.mkdirSync(downloads, { recursive: true });
    // The browser saves the page's Blob here (the record never leaves the machine).
    await cdp("Browser.setDownloadBehavior", {
      behavior: "allow",
      downloadPath: downloads,
      eventsEnabled: true,
    });
    say(`choose ROM ${path.basename(rom)} (read by the browser, not served)`);
    const doc = await cdp("DOM.getDocument", { depth: 0 });
    const input = await cdp("DOM.querySelector", { nodeId: doc.root.nodeId, selector: "#rom" });
    await cdp("DOM.setFileInputFiles", { files: [rom], nodeId: input.nodeId });
    for (let i = 0; i < 2400 && !(await pageText("#recording"))?.includes("recording"); i++) {
      await sleep(250);
    }
    if (!(await pageText("#recording"))?.includes("recording")) {
      throw new Error("the page never started recording");
    }
    say(`recording: ${await pageText("#recording")}`);
    await evaluate("document.querySelector('#dq9').focus()");
    await sleep(1500);
    // A short hand: read the opening, walk about, turn the camera, open the field menu.
    for (let i = 0; i < 14; i++) await tap("Enter");
    await hold("ArrowDown", 900);
    await hold("ArrowLeft", 500);
    for (let i = 0; i < 6; i++) await tap("Space");
    await hold("ArrowUp", 700);
    await hold("ArrowRight", 600);
    await tap("KeyQ", 0);
    await tap("Enter");
    await tap("KeyI", 400);
    for (let i = 0; i < 4; i++) await tap("ArrowDown", 200);
    await tap("Enter", 300);
    // Cancel inside the menu, then Escape in the field: on the page Escape must not stop the
    // session (it closes a native window, `App::key`), or the recording and its download die
    // with the loop.
    await tap("KeyX", 300);
    const beforeEscape = await stepsShown();
    await tap("Escape", 600);
    await sleep(1200);
    if ((await stepsShown()) <= beforeEscape) {
      throw new Error("Escape stopped the page's session");
    }
    say(`Escape left the session running (${await stepsShown()} steps)`);
    for (let i = 0; i < 10; i++) await tap("Enter");
    await hold("ArrowDown", 1200);
    for (let i = 0; i < 8; i++) await tap("Enter");
    // Walk until the session is long enough (the page counts its own steps).
    const until = Date.now() + maxWall;
    let lastSteps = 0;
    let lastMove = Date.now();
    while ((await stepsShown()) < target && Date.now() < until) {
      const steps = await stepsShown();
      if (steps > lastSteps) {
        lastSteps = steps;
        lastMove = Date.now();
      } else if (Date.now() - lastMove > stallLimit) {
        throw new Error(`the recording stopped at ${steps} steps (Escape ends the session)`);
      }
      await hold("ArrowLeft", 600);
      await tap("Enter");
      await sleep(500);
    }
    const steps = await stepsShown();
    say(`recorded ${steps} steps (target ${target}); saving through the page's button`);
    await shot("recording-indicator");
    await evaluate("document.querySelector('#dq9').focus()");
    await evaluate("document.querySelector('#recordsave').click()");
    for (let i = 0; i < 240 && !recordFile; i++) {
      const found = fs.existsSync(downloads)
        ? fs.readdirSync(downloads).filter((name) => name.endsWith(".dq9rec"))
        : [];
      if (found.length) recordFile = path.join(downloads, found[0]);
      else await sleep(250);
    }
    if (!recordFile) throw new Error("the page's download never appeared");
    const bytes = fs.readFileSync(recordFile);
    const header = bytes.toString("utf8").split("\n")[0];
    say(`record ${path.basename(recordFile)}: ${bytes.length} bytes, ${header}`);
    stop = (await pageText("#status")) ?? null;
    say(`status: ${stop}`);
    ok = /Record saved/.test(stop ?? "");
    fs.writeFileSync(path.join(out, "result.json"), JSON.stringify({
      ok,
      mode,
      url,
      browser: version.Browser,
      wallSeconds: (Date.now() - started) / 1000,
      recordFile,
      recordBytes: bytes.length,
      header,
      steps,
      status: stop,
      consoleErrors,
    }, null, 2));
  } else {
    const record = path.resolve(need("record"));
    const bytes = fs.readFileSync(record);
    say(`choose record ${path.basename(record)} (${bytes.length} bytes) then the ROM`);
    await pickFile("#recordfile", record);
    await sleep(750);
    say(`replaying: ${await pageText("#recording")}`);
    await pickFile("#rom", rom);
    const until = Date.now() + maxWall;
    let last = "";
    while (Date.now() < until) {
      const text = (await pageText("#status")) ?? "";
      if (text !== last) {
        last = text;
        say(`status: ${text}`);
      }
      if (/stopped:/.test(text)) {
        stop = text;
        break;
      }
      await sleep(500);
    }
    await shot("replay-end");
    // "record ended" means every due digest matched: `Replay` stops at the first difference.
    ok = !!stop && !/mismatch/.test(stop);
    fs.writeFileSync(path.join(out, "result.json"), JSON.stringify({
      ok,
      mode,
      url,
      browser: version.Browser,
      wallSeconds: (Date.now() - started) / 1000,
      record,
      recordBytes: bytes.length,
      stop,
      consoleErrors,
    }, null, 2));
  }
  say(`result: ${JSON.stringify({ ok, mode, stop, consoleErrors })}`);
  await new Promise((resolve) => consoleLog.end(resolve));
  await new Promise((resolve) => progressLog.end(resolve));
  stopBrowser();
  server?.close();
  process.exit(ok ? 0 : 1);
}

// ---- macro mode: field captures (`--mode shots`) ----
// Drives the page's field from a small plan file, one action per line, so a capture session
// chooses the ROM and the optional `--save` through the page's own file inputs, switches the
// page's `Screens`/`Layout` selectors and presses real keys:
//
//   wait <ms>              sleep (the field needs a few frames after the ROM/save is read)
//   focus                  give the keyboard back to the canvas (`#dq9`)
//   tap <Key> [gapMs]      press and release a key
//   hold <Key> <ms>        hold a key
//   shot <name> [canvas]   screenshot the page, or only the `#dq9` canvas
//   screens <port|original>  the page's `Screens` selector (`?screens=`)
//   layout <name>          the page's `Layout` selector (`?layout=`)
//
// The query still carries the start options (`?map=M01&screens=original`); the save starts the
// field itself (`set_save`), so a save session needs no "New game" click.
if (mode === "shots") {
  const planFile = args.script ? path.resolve(args.script) : null;
  if (!planFile) throw new Error("missing --script (the capture plan)");
  const plan = fs
    .readFileSync(planFile, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"))
    .map((line) => line.split(/\s+/));
  const save = args.save ? path.resolve(args.save) : null;
  const pickFile = async (selector, file) => {
    const doc = await cdp("DOM.getDocument", { depth: 0 });
    const node = await cdp("DOM.querySelector", { nodeId: doc.root.nodeId, selector });
    if (!node.nodeId) throw new Error(`no ${selector} field in the page`);
    await cdp("DOM.setFileInputFiles", { files: [file], nodeId: node.nodeId });
  };
  const virtualKeys = {
    Enter: 13, Space: 32, ArrowUp: 38, ArrowDown: 40, ArrowLeft: 37, ArrowRight: 39,
    KeyW: 87, KeyA: 65, KeyS: 83, KeyD: 68, KeyQ: 81, KeyE: 69, KeyZ: 90, KeyX: 88,
    KeyI: 73, Escape: 27,
  };
  const keyText = (name) =>
    name === "Space" ? " " : name.startsWith("Key") ? name.slice(3).toLowerCase() : name;
  const keyEvent = (type, name) =>
    cdp("Input.dispatchKeyEvent", {
      type,
      key: keyText(name),
      code: name,
      windowsVirtualKeyCode: virtualKeys[name] ?? 0,
      nativeVirtualKeyCode: virtualKeys[name] ?? 0,
    });
  const tap = async (name, gap = 300) => {
    await keyEvent("keyDown", name);
    await sleep(60);
    await keyEvent("keyUp", name);
    await sleep(gap);
  };
  const hold = async (name, ms) => {
    await keyEvent("keyDown", name);
    await sleep(ms);
    await keyEvent("keyUp", name);
    await sleep(200);
  };
  const selectValue = async (selector, value) => {
    const got = await evaluate(
      `(() => { const el = document.querySelector(${JSON.stringify(selector)});
        if (!el) return "no selector";
        el.value = ${JSON.stringify(value)};
        if (el.value !== ${JSON.stringify(value)}) return "rejected: " + el.value;
        el.dispatchEvent(new Event("change", { bubbles: true }));
        return el.value; })()`,
    );
    if (got !== value) throw new Error(`${selector}=${value}: ${got}`);
    say(`${selector}=${got}`);
  };
  const taken = [];
  const capture = async (name, canvasOnly) => {
    const params = { format: "png" };
    if (canvasOnly) {
      const rect = JSON.parse(
        await evaluate(`(() => { const el = document.querySelector('#dq9');
          const r = el.getBoundingClientRect();
          return JSON.stringify({ x: r.x, y: r.y, width: r.width, height: r.height }); })()`),
      );
      params.clip = { ...rect, scale: 1 };
    }
    const png = await cdp("Page.captureScreenshot", params);
    const file = `shots/${String(taken.length + 1).padStart(2, "0")}-${name}.png`;
    fs.writeFileSync(path.join(out, file), Buffer.from(png.data, "base64"));
    taken.push({ file, name, canvasOnly });
    say(`screenshot ${file}${canvasOnly ? " (canvas)" : ""}`);
  };

  const url = hosted ? hosted.toString() : `http://127.0.0.1:${port}/?${query}`;
  say(`open ${url}`);
  await cdp("Page.navigate", { url });
  let ready = false;
  for (let i = 0; i < 600 && !ready; i++) {
    ready = await evaluate(
      "(() => { const r = document.querySelector('#rom'); return !!r && !r.disabled; })()",
    ).catch(() => false);
    if (!ready) await sleep(250);
  }
  if (!ready) throw new Error("the page never offered the ROM field");
  say(`choose ROM ${path.basename(rom)} (read by the browser, not served)`);
  await pickFile("#rom", rom);
  // The page disables a file field while it reads it, then re-enables it (`web.rs`). The status
  // line is overwritten at once by the engine's own lines, so the field state is the signal.
  const waitRead = async (selector, what) => {
    for (let i = 0; i < 60; i++) {
      const disabled = await evaluate(
        `(() => !!document.querySelector(${JSON.stringify(selector)})?.disabled)()`,
      );
      if (disabled) break;
      await sleep(100);
    }
    for (let i = 0; i < 1200; i++) {
      const disabled = await evaluate(
        `(() => !!document.querySelector(${JSON.stringify(selector)})?.disabled)()`,
      );
      if (!disabled) return;
      await sleep(100);
    }
    throw new Error(`${what}: the field stayed disabled`);
  };
  await waitRead("#rom", "the ROM");
  if (save) {
    say(`choose save ${path.basename(save)} (read by the browser, not served)`);
    await pickFile("#save", save);
    await waitRead("#save", "the save");
  }
  // The field builds its map and assets over the next frames; the plan's first `wait` covers
  // the rest (it captures whatever the page shows, so a short wait cannot fake a scene).
  await sleep(2500);
  for (const [action, ...rest] of plan) {
    if (action === "wait") await sleep(Number(rest[0] ?? 1000));
    else if (action === "focus") await evaluate("document.querySelector('#dq9').focus()");
    else if (action === "tap") await tap(rest[0] ?? "Enter", Number(rest[1] ?? 300));
    else if (action === "hold") await hold(rest[0] ?? "Enter", Number(rest[1] ?? 500));
    else if (action === "shot") await capture(rest[0] ?? "shot", rest[1] === "canvas");
    else if (action === "screens") await selectValue("#screens", rest[0] ?? "original");
    else if (action === "layout") await selectValue("#layout", rest[0] ?? "stacked");
    else throw new Error(`unknown plan action: ${action}`);
  }
  const summary = {
    ok: !crashed && taken.length > 0,
    mode,
    url,
    query,
    save,
    script: planFile,
    browser: version.Browser,
    wallSeconds: (Date.now() - started) / 1000,
    consoleErrors,
    shots: taken,
  };
  fs.writeFileSync(path.join(out, "result.json"), JSON.stringify(summary, null, 2));
  say(`result: ${JSON.stringify({ ok: summary.ok, shots: taken.length, consoleErrors })}`);
  await new Promise((resolve) => consoleLog.end(resolve));
  await new Promise((resolve) => progressLog.end(resolve));
  stopBrowser();
  server?.close();
  process.exit(summary.ok ? 0 : 1);
}

// ---- screenshots ----
const shots = [];
let shotQueue = Promise.resolve();
const shoot = (name, delay = 0) => {
  const index = String(shots.length + 1).padStart(2, "0");
  const file = `shots/${index}-${name}.png`;
  shots.push({ name, file, at: wall().trim(), session: null });
  shotQueue = shotQueue.then(async () => {
    if (delay) await sleep(delay);
    try {
      const shot = await cdp("Page.captureScreenshot", { format: "png" });
      fs.writeFileSync(path.join(out, file), Buffer.from(shot.data, "base64"));
      const state = await evaluate("window.dq9_session ? window.dq9_session.line : null").catch(() => null);
      shots.find((s) => s.file === file).session = state;
      say(`screenshot ${file}: ${state ?? ""}`);
    } catch (e) {
      say(`screenshot ${file} failed: ${e.message}`);
    }
  });
};

// ---- milestones ----
const done = new Set();
const once = (key, name, delay = 0) => {
  if (done.has(key)) return;
  done.add(key);
  shoot(name, delay);
};
let lostShots = 0;
let hexagoonScenes = 0;
const showBattles = /(^|&)battles=show(&|$)/.test(query);
consoleHooks.push((_type, text) => {
  if (/^roamers: contact with monster/.test(text)) once("roamer", "roaming-contact");
  if (/^encounter: /.test(text)) once("roamer-fought", "roaming-fight-settled");
  // The scene is only on screen with battles=show; otherwise it is built and left at once.
  // The first shown fight: its entrance, then the windows of the fight itself.
  if (showBattles && /^battle \S+ on stage .*fighters/.test(text)) {
    once("scene", "battle-scene");
    once("scene-windows", "battle-scene-windows", 2000);
  }
  // The Hexagoon (set battle 2): entrance, windows, and one shot per lost attempt at its end.
  if (showBattles && /^battle 2 on stage/.test(text)) {
    const attempt = ++hexagoonScenes;
    shoot(`hexagoon-scene-${attempt}`);
    shoot(`hexagoon-windows-${attempt}`, 2000);
    shoot(`hexagoon-windows-${attempt}-later`, 6000);
  }
  if (/^battle 2: Lost/.test(text)) shoot(`hexagoon-lost-${++lostShots}`);
  if (/^battle 2: Won/.test(text)) once("hexagoon-won", "hexagoon-won");
});
const milestone = (line) => {
  let checkpoint;
  try {
    checkpoint = JSON.parse(line);
  } catch {
    return;
  }
  if (checkpoint.kind === "control" && checkpoint.map === 4105) once("x01m05", "first-control-X01M05");
  if (checkpoint.map === 1100) once("angel-falls", "angel-falls-return");
  if (checkpoint.map === 7104 || checkpoint.map === 7105) once("hexagon", "hexagon");
  if (checkpoint.kind === "event_start" && checkpoint.event === 2750) once("2750", "event-2750");
};

// ---- the page and the ROM ----
const url = hosted ? hosted.toString() : `http://127.0.0.1:${port}/?${query}`;
say(`open ${url}`);
await cdp("Page.navigate", { url });
for (let i = 0; i < 600; i++) {
  const ready = await evaluate(
    "(() => { const r = document.querySelector('#rom'); return !!r && !r.disabled; })()",
  ).catch(() => false);
  if (ready) break;
  await sleep(250);
}
const document = await cdp("DOM.getDocument", { depth: 0 });
const input = await cdp("DOM.querySelector", { nodeId: document.root.nodeId, selector: "#rom" });
say(`choose ROM ${path.basename(rom)} (read by the browser, not served)`);
await cdp("DOM.setFileInputFiles", { files: [rom], nodeId: input.nodeId });

// ---- follow the session ----
let seen = 0;
let lastTick = -1;
let lastMove = Date.now();
let lastLine = "";
let result = null;
// Takes the checkpoints after `from` and their number in one evaluation (the run goes on
// between two calls, so the count of an earlier poll must not be reused).
const pullCheckpoints = async (from) => {
  const got = JSON.parse(
    await evaluate(`(() => { const a = window.dq9_session.checkpoints;
      return JSON.stringify({ n: a.length, lines: a.slice(${from}).join("\\n") }); })()`),
  );
  checkpointFile.write(got.lines + "\n");
  for (const line of got.lines.split("\n")) milestone(line);
  return got.n;
};
while (true) {
  await sleep(1000);
  if (crashed) {
    result = { ok: false, why: "the page crashed" };
    break;
  }
  if (Date.now() - started > maxWall) {
    result = { ok: false, why: `the wall limit of ${maxWall / 1000} s was reached` };
    break;
  }
  let state;
  try {
    state = await evaluate(`(() => {
      const s = window.dq9_session;
      const status = document.querySelector('#status');
      return JSON.stringify({
        have: !!s, tick: s ? s.tick : 0, seconds: s ? s.seconds : 0, wall: s ? s.wall : 0,
        line: s ? s.line : null, done: s ? s.done : false, stop: s ? s.stop : null,
        count: s ? s.checkpoints.length : 0,
        status: status ? status.textContent : null, kind: status ? status.dataset.kind : null,
      });
    })()`);
  } catch (e) {
    say(`evaluate failed: ${e.message}`);
    continue;
  }
  state = JSON.parse(state);
  if (!state.have) {
    if (state.kind === "error") {
      result = { ok: false, why: `page error: ${state.status}` };
      break;
    }
    continue;
  }
  if (state.count > seen) seen = await pullCheckpoints(seen);
  if (state.tick !== lastTick) {
    lastTick = state.tick;
    lastMove = Date.now();
  } else if (Date.now() - lastMove > stallLimit && !state.done) {
    result = { ok: false, why: `no progress for ${stallLimit / 1000} s at tick ${state.tick}` };
    break;
  }
  if (state.tick >= 240) once("opening", "new-game-opening");
  if (state.line && state.line !== lastLine) {
    lastLine = state.line;
    say(`${state.line} [wall in page ${state.wall.toFixed(0)} s, ${(state.tick / Math.max(state.wall, 1)).toFixed(0)} steps/s]`);
  }
  if (state.done) {
    result = { ok: /event \d+ ended/.test(state.stop ?? ""), why: state.stop };
    break;
  }
}

// ---- end ----
const finalState = await evaluate(`(() => {
  const s = window.dq9_session;
  return JSON.stringify(s ? { report: s.report, stop: s.stop, tick: s.tick, attempts: s.attempts, count: s.checkpoints.length } : null);
})()`).catch(() => "null");
const final = JSON.parse(finalState);
if (final) {
  if (final.count > seen) seen = await pullCheckpoints(seen);
  final.count = seen;
  if (final.report) fs.writeFileSync(path.join(out, "report.txt"), final.report);
}
shoot("final");
await shotQueue;
const summary = {
  ok: result?.ok ?? false,
  why: result?.why ?? null,
  url,
  browser: version.Browser,
  wallSeconds: (Date.now() - started) / 1000,
  gameTicks: final?.tick ?? null,
  gameSeconds: final ? final.tick / 60 : null,
  checkpoints: final?.count ?? 0,
  consoleErrors,
  attempts: final?.attempts ?? [],
  shots,
};
fs.writeFileSync(path.join(out, "result.json"), JSON.stringify(summary, null, 2));
say(`result: ${JSON.stringify({ ok: summary.ok, why: summary.why, consoleErrors })}`);
await new Promise((resolve) => consoleLog.end(resolve));
await new Promise((resolve) => checkpointFile.end(resolve));
stopBrowser();
server?.close();
process.exit(summary.ok ? 0 : 1);
