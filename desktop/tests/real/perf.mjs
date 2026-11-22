// Performance budget check on the real, release-built Hub (Linux, under xvfb):
//
//   1. pairs a simulated phone over real QUIC/TLS/PAKE (same flow as smoke.mjs),
//   2. the phone watches the Hub's real screen at 1280 px / 30 fps and reports the
//      capture-to-display latency of 150 frames, then stops,
//   3. with the phone still connected and the dashboard open, measures the Hub's CPU over
//      60 s and its memory — the app plus its WebKit processes (PSS, so shared libraries
//      are counted once).
//
//   node tests/real/perf.mjs <release devti-desktop> <sim_device> [results.json]
//
// Budgets (from the build brief): idle CPU < 2 % of one core, RAM < 250 MB, latency < 50 ms.

import { spawn } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [app, sim, out = join(tmpdir(), "devti-perf.json")] = process.argv.slice(2);
if (!app || !sim) {
  console.error("usage: perf.mjs <devti-desktop> <sim_device> [results.json]");
  process.exit(2);
}
const BUDGET = { idleCpuPercent: 2, memoryMB: 250, latencyMs: 50 };
const home = mkdtempSync(join(tmpdir(), "devti-perf-"));
const env = {
  ...process.env,
  HOME: home,
  XDG_DATA_HOME: join(home, "data"),
  XDG_CONFIG_HOME: join(home, "cfg"),
};
const driver = spawn("tauri-driver", ["--port", "4445", "--native-port", "4446"], { env, stdio: "inherit" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const W = "http://127.0.0.1:4445";
let sid;

async function wd(method, path, body) {
  const res = await fetch(`${W}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = await res.json();
  if (json.value?.error) throw new Error(`${path}: ${json.value.error} ${json.value.message}`);
  return json.value;
}
const s = (path) => `/session/${sid}${path}`;
const ELEM = "element-6066-11e4-a52e-4f735466cecc";
async function find(xpath, timeout = 15000) {
  const end = Date.now() + timeout;
  for (;;) {
    try {
      const el = await wd("POST", s("/element"), { using: "xpath", value: xpath });
      return el[ELEM] ?? el.ELEMENT ?? Object.values(el)[0];
    } catch (e) {
      if (Date.now() > end) throw new Error(`not found: ${xpath} (${e.message})`);
      await sleep(250);
    }
  }
}
const button = (text) =>
  `//button[normalize-space(.)="${text}"] | //*[@role="button" and normalize-space(.)="${text}"]`;
async function click(xpath) {
  await wd("POST", s(`/element/${await find(xpath)}/click`), {});
  await sleep(450);
}
async function invoke(cmd, args = {}) {
  return wd("POST", s("/execute/async"), {
    script:
      "const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then(done, (e) => done({ error: String(e) }));",
    args: [cmd, args],
  });
}

// ---- /proc helpers: the Hub process and everything it started
const stat = (pid) => {
  const raw = readFileSync(`/proc/${pid}/stat`, "utf8");
  const rest = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
  return { ppid: Number(rest[1]), ticks: Number(rest[11]) + Number(rest[12]) };
};
const pids = () =>
  readdirSync("/proc")
    .filter((d) => /^\d+$/.test(d))
    .map(Number);
function tree(root) {
  const all = new Map();
  for (const p of pids()) {
    try {
      all.set(p, stat(p).ppid);
    } catch {}
  }
  const out = [root];
  for (let i = 0; i < out.length; i++) for (const [p, pp] of all) if (pp === out[i]) out.push(p);
  return out;
}
const cmdline = (pid) => {
  try {
    return readFileSync(`/proc/${pid}/cmdline`, "utf8").replaceAll("\0", " ").trim();
  } catch {
    return "";
  }
};
function kb(pid, field, file) {
  try {
    const m = readFileSync(`/proc/${pid}/${file}`, "utf8").match(new RegExp(`^${field}:\\s+(\\d+) kB`, "m"));
    return m ? Number(m[1]) : 0;
  } catch {
    return 0;
  }
}

let simProc;
let simOut = "";
const waitSim = async (text, ms = 15000) => {
  const end = Date.now() + ms;
  while (!simOut.includes(text)) {
    if (Date.now() > end) throw new Error(`simulated phone never printed: ${text}`);
    await sleep(200);
  }
};

let failed = false;
try {
  for (let i = 0; i < 40; i++) {
    try {
      await fetch(`${W}/status`);
      break;
    } catch {
      await sleep(250);
    }
  }
  sid = (
    await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } })
  ).sessionId;
  await click(button("Set up Devti"));
  await click(button("Continue"));
  await click(button("Continue"));
  await find(`//*[contains(., "Scan with your phone")]`);
  const snap = await invoke("snapshot");
  simProc = spawn(
    sim,
    [
      "--uri",
      snap.window.uri,
      "--name",
      "Perf phone",
      "--seconds",
      "400",
      "--watch",
      "--watch-width",
      "1280",
    ].concat(["--watch-fps", "30", "--watch-frames", process.env.DEVTI_PERF_FRAMES || "150"]),
    { stdio: ["ignore", "pipe", "inherit"] },
  );
  simProc.stdout.on("data", (d) => {
    simOut += d;
    process.stdout.write(`[sim] ${d}`);
  });
  await find(`//*[contains(., "show this code?")]`, 20000);
  await click(button("The codes match"));
  await find(`//*[contains(., "joined your ecosystem")]`, 20000);
  await sleep(800);
  await click(`(${button("Continue")})[1]`);
  await sleep(2500);
  await click(button("Continue"));
  await click(button("Skip the tour"));
  await click(button("Open my ecosystem"));
  await sleep(3000);

  // Something changes on screen all the time, as in real use (unchanged frames are never
  // sent): a running clock drawn by the page itself.
  await wd("POST", s("/execute/sync"), {
    script: `const el = document.createElement("div");
      el.id = "perf-clock";
      el.style.cssText = "position:fixed;right:24px;bottom:24px;font:600 48px monospace;color:#fff;z-index:99999";
      document.body.appendChild(el);
      const tick = () => { el.textContent = performance.now().toFixed(1); requestAnimationFrame(tick); };
      tick();`,
    args: [],
  });
  await waitSim("screen latency", 180000);
  await wd("POST", s("/execute/sync"), {
    script: "document.getElementById('perf-clock')?.remove();",
    args: [],
  });
  const m = simOut.match(
    /screen latency median=(\d+) p95=(\d+) fps=([\d.]+) frames=(\d+) size=(\d+)x(\d+) kb=(\d+)/,
  );
  if (!m) throw new Error("no latency line");
  const latency = {
    medianMs: +m[1],
    p95Ms: +m[2],
    fps: +m[3],
    frames: +m[4],
    size: `${m[5]}×${m[6]}`,
    kbPerFrame: +m[7],
  };
  console.log(
    `PERF: screen latency median ${latency.medianMs} ms, p95 ${latency.p95Ms} ms at ${latency.fps} fps (${latency.size})`,
  );

  // Idle: phone connected, dashboard open, nobody touching anything.
  await sleep(15000);
  const hub = pids().find((p) => cmdline(p).startsWith(app));
  if (!hub) throw new Error("Hub process not found");
  const procs = tree(hub);
  const t0 = procs.reduce(
    (a, p) =>
      a +
      (() => {
        try {
          return stat(p).ticks;
        } catch {
          return 0;
        }
      })(),
    0,
  );
  await sleep(60000);
  const t1 = tree(hub).reduce(
    (a, p) =>
      a +
      (() => {
        try {
          return stat(p).ticks;
        } catch {
          return 0;
        }
      })(),
    0,
  );
  const hz = 100; // USER_HZ on Linux
  const idleCpuPercent = +(((t1 - t0) / hz / 60) * 100).toFixed(2);
  const now = tree(hub);
  const pssMB = +(now.reduce((a, p) => a + kb(p, "Pss", "smaps_rollup"), 0) / 1024).toFixed(1);
  const rssMB = +(now.reduce((a, p) => a + kb(p, "VmRSS", "status"), 0) / 1024).toFixed(1);
  const processes = now.map((p) => ({
    pid: p,
    cmd: cmdline(p).split(" ")[0].split("/").pop(),
    pssMB: +(kb(p, "Pss", "smaps_rollup") / 1024).toFixed(1),
  }));
  console.log(
    `PERF: idle CPU ${idleCpuPercent} % of one core over 60 s; memory ${pssMB} MB PSS (${rssMB} MB RSS) across ${now.length} processes`,
  );
  for (const p of processes) console.log(`      ${p.cmd} ${p.pssMB} MB`);

  const results = {
    date: new Date().toISOString(),
    budget: BUDGET,
    latency,
    idleCpuPercent,
    memory: { pssMB, rssMB, processes },
    pass: {
      latency: latency.medianMs < BUDGET.latencyMs,
      cpu: idleCpuPercent < BUDGET.idleCpuPercent,
      memory: pssMB < BUDGET.memoryMB,
    },
  };
  writeFileSync(out, JSON.stringify(results, null, 2));
  for (const [k, ok] of Object.entries(results.pass)) {
    console.log(`${ok ? "PASS" : "OVER BUDGET"}: ${k}`);
    if (!ok) failed = true;
  }
} catch (e) {
  failed = true;
  console.error("FAIL:", e.message);
} finally {
  simProc?.kill();
  try {
    await wd("DELETE", s(""));
  } catch {}
  driver.kill();
  process.exit(failed ? 1 : 0);
}
