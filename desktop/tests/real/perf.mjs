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
      "--watch-fps",
      "30",
      "--watch-frames",
      process.env.DEVTI_PERF_FRAMES || "150",
      // A phone at rest reports a new battery level every few minutes, not every 5 s.
      "--battery-every",
      "120",
    ],
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
      window.__perfClock = true;
      const tick = () => { if (!window.__perfClock) return; el.textContent = performance.now().toFixed(1); requestAnimationFrame(tick); };
      tick();`,
    args: [],
  });
  await waitSim("screen latency", 180000);
  await wd("POST", s("/execute/sync"), {
    script: "window.__perfClock = false; document.getElementById('perf-clock')?.remove();",
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
  const running = await wd("POST", s("/execute/sync"), {
    script: `return { idle: document.documentElement.dataset.idle, animations: document.getAnimations()
      .filter((a) => a.playState === "running")
      .map((a) => (a.animationName || a.transitionProperty || a.id || "animation") + " on " +
        String(a.effect?.target?.className?.baseVal ?? a.effect?.target?.className ?? a.effect?.target?.tagName).slice(0, 60)) };`,
    args: [],
  });
  console.log(`      idle=${running.idle}; running animations: ${running.animations.join(" | ") || "none"}`);
  const churn = await wd("POST", s("/execute/async"), {
    script: `const done = arguments[0];
      let raf = 0; const orig = window.requestAnimationFrame;
      const stacks = new Map();
      window.requestAnimationFrame = (cb) => { raf++;
        const st = (new Error().stack || "").split("\\n").slice(1, 5).map((l) => l.replace(/tauri:\\/\\/localhost\\//, "")).join(" < ");
        stacks.set(st, (stacks.get(st) ?? 0) + 1); return orig(cb); };
      const seen = new Map(); let n = 0;
      const mo = new MutationObserver((list) => { for (const m of list) { n++;
        const el = m.target.nodeType === 1 ? m.target : m.target.parentElement;
        const key = (el?.className?.baseVal ?? el?.className ?? el?.tagName ?? "?") + "";
        seen.set(key.slice(0, 50), (seen.get(key.slice(0, 50)) ?? 0) + 1); } });
      mo.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
      setTimeout(() => { mo.disconnect(); window.requestAnimationFrame = orig;
        done({ raf, mutations: n, top: [...seen].sort((a, b) => b[1] - a[1]).slice(0, 6),
          rafBy: [...stacks].sort((a, b) => b[1] - a[1]).slice(0, 3) }); }, 5000);`,
    args: [],
  });
  for (const [st, c] of churn.rafBy ?? []) console.log(`      rAF ×${c}: ${st.slice(0, 400)}`);
  console.log(
    `      in 5 s: ${churn.raf} animation frames requested, ${churn.mutations} DOM mutations (${churn.top.map(([k, v]) => `${k}×${v}`).join(", ")})`,
  );
  const ticksOf = (p) => {
    try {
      return stat(p).ticks;
    } catch {
      return 0;
    }
  };
  const threadTicks = () => {
    const out = new Map();
    for (const t of readdirSync(`/proc/${hub}/task`)) {
      try {
        const raw = readFileSync(`/proc/${hub}/task/${t}/stat`, "utf8");
        const name = raw.slice(raw.indexOf("(") + 1, raw.lastIndexOf(")"));
        const rest = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
        out.set(`${name}#${t}`, Number(rest[11]) + Number(rest[12]));
      } catch {}
    }
    return out;
  };
  const per0 = new Map(tree(hub).map((p) => [p, ticksOf(p)]));
  const th0 = threadTicks();
  const t0 = [...per0.values()].reduce((a, b) => a + b, 0);
  await sleep(60000);
  const per1 = new Map(tree(hub).map((p) => [p, ticksOf(p)]));
  const th1 = threadTicks();
  const t1 = [...per1.values()].reduce((a, b) => a + b, 0);
  for (const [p, v] of per1) {
    const pct = (((v - (per0.get(p) ?? 0)) / 100 / 60) * 100).toFixed(2);
    console.log(`      cpu ${cmdline(p).split(" ")[0].split("/").pop()}: ${pct} %`);
  }
  const busy = [...th1]
    .map(([k, v]) => [k, v - (th0.get(k) ?? 0)])
    .sort((a, b) => b[1] - a[1])
    .slice(0, 6);
  console.log(`      busiest Hub threads (ticks in 60 s): ${busy.map(([k, v]) => `${k}=${v}`).join(", ")}`);
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
  // Where each process's memory goes (largest mappings by PSS), to see what's reducible.
  for (const p of now) {
    const by = new Map();
    let name = "[anon]";
    try {
      for (const line of readFileSync(`/proc/${p}/smaps`, "utf8").split("\n")) {
        const head = line.match(/^[0-9a-f]+-[0-9a-f]+ \S+ \S+ \S+ \S+\s*(.*)$/);
        if (head)
          name =
            head[1]
              ?.split("/")
              .pop()
              ?.replace(/\.so.*/, "") || "[anon]";
        else if (line.startsWith("Pss:")) by.set(name, (by.get(name) ?? 0) + Number(line.split(/\s+/)[1]));
      }
    } catch {}
    const top = [...by]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 6)
      .map(([k, v]) => `${k} ${(v / 1024).toFixed(1)}`);
    console.log(`      ${cmdline(p).split(" ")[0].split("/").pop()}: ${top.join(", ")} MB`);
  }

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
