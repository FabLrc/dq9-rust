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
fs.rmSync(profile, { recursive: true, force: true });
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
