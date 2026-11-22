// Real-app smoke test: drives the actual Devti Hub binary (WebKitGTK + Rust core) through
// WebDriver (tauri-driver), pairs it with a simulated phone over real QUIC/TLS/PAKE,
// and checks the dashboard. Linux only; run under a display (xvfb-run).
//
//   node tests/real/smoke.mjs <path-to-devti-desktop> <path-to-sim_device> [screenshot-dir]

import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [app, sim, shots = join(tmpdir(), "devti-smoke")] = process.argv.slice(2);
if (!app || !sim) {
  console.error("usage: smoke.mjs <devti-desktop> <sim_device> [screenshot-dir]");
  process.exit(2);
}
mkdirSync(shots, { recursive: true });
const home = mkdtempSync(join(tmpdir(), "devti-home-"));
const env = {
  ...process.env,
  HOME: home,
  XDG_DATA_HOME: join(home, "data"),
  XDG_CONFIG_HOME: join(home, "cfg"),
};

const driver = spawn("tauri-driver", ["--port", "4444"], { env, stdio: "inherit" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const W = "http://127.0.0.1:4444";
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
  const id = await find(xpath);
  await wd("POST", s(`/element/${id}/click`), {});
  await sleep(450);
}
async function shot(name) {
  const png = await wd("GET", s("/screenshot"));
  writeFileSync(join(shots, `${name}.png`), Buffer.from(png, "base64"));
}
async function invoke(cmd, args = {}) {
  return wd("POST", s("/execute/async"), {
    script:
      "const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then(done, (e) => done({ error: String(e) }));",
    args: [cmd, args],
  });
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
  const session = await wd("POST", "/session", {
    capabilities: { alwaysMatch: { "tauri:options": { application: app } } },
  });
  sid = session.sessionId;
  await find(button("Set up Devti"), 30000);
  await sleep(2200);
  await shot("01-welcome");
  await click(button("Set up Devti"));
  await shot("02-permissions");
  await click(button("Continue"));
  await click(button("Continue"));
  await find(`//*[contains(., "Scan with your phone")]`);
  await sleep(600);
  await shot("03-pair");

  const snap = await invoke("snapshot");
  if (!snap?.window?.uri) throw new Error(`no pairing window: ${JSON.stringify(snap)}`);
  console.log("hub:", snap.local.info.name, snap.local.info.id, "window", snap.window.addresses);

  // A 3 MiB file the simulated phone sends once it is connected.
  const payload = join(home, "from-the-phone.bin");
  writeFileSync(payload, Buffer.alloc(3 * 1024 * 1024, 7));
  simProc = spawn(
    sim,
    ["--uri", snap.window.uri, "--name", "Pixel 8 (sim)", "--seconds", "180", "--send", payload, "--watch"],
    { stdio: ["ignore", "pipe", "inherit"] },
  );
  simProc.stdout.on("data", (d) => {
    simOut += d;
    process.stdout.write(`[sim] ${d}`);
  });
  await find(`//*[contains(., "show this code?")]`, 20000);
  await sleep(500);
  await shot("04-verify");
  await click(button("The codes match"));
  await find(`//*[contains(., "joined your ecosystem")]`, 20000);
  await sleep(800);
  await shot("05-joined");
  await click(`(${button("Continue")})[1]`);
  await sleep(2500);
  await shot("06-bluetooth");
  await click(button("Continue"));
  await click(button("Skip the tour"));
  await click(button("Open my ecosystem"));
  await sleep(6500);
  await shot("07-dashboard");

  const after = await invoke("snapshot");
  const peer = after.peers[0];
  if (!peer?.online) throw new Error(`peer not online: ${JSON.stringify(after.peers)}`);
  if (!peer.status.battery) throw new Error("no battery reported by the simulated phone");
  console.log(
    `PASS: ${peer.info.name} online, battery ${peer.status.battery.percent}%, rtt ${peer.connection.rtt_ms} ms`,
  );

  // Phase 2 over the real stack.
  const waitFor = async (what, check, ms = 20000) => {
    const end = Date.now() + ms;
    for (;;) {
      const v = await check();
      if (v) return v;
      if (Date.now() > end) throw new Error(`timed out: ${what}`);
      await sleep(400);
    }
  };
  await waitFor("mirrored notification", async () =>
    (await invoke("snapshot")).notifications.some((n) => n.notif.text === "Hello from the simulator"),
  );
  console.log("PASS: phone notification mirrored to the Hub");
  const received = await waitFor("file received", async () =>
    (await invoke("transfers")).find((t) => t.incoming && t.state === "done" && t.size === 3 * 1024 * 1024),
  );
  console.log(`PASS: received ${received.name} (${received.size} bytes)`);
  await invoke("share_clipboard", { text: "hello from the real hub" });
  await waitSim("clipboard from");
  console.log("PASS: clipboard reached the phone");
  await invoke("ring", { id: peer.info.id, on: true });
  await waitSim("ring from");
  console.log("PASS: find my device rang the phone");
  const key = (await invoke("snapshot")).notifications[0].notif.key;
  await invoke("notification_action", { id: peer.info.id, key, action: "reply", reply: "On my way" });
  await waitSim("reply=On my way");
  console.log("PASS: quick reply reached the phone");
  // Phase 3: a saved command on the Hub, run from the phone; output comes back.
  await invoke("save_commands", {
    commands: [{ id: "hello", name: "Say hello", command: "echo hello from the hub", confirm: false }],
  });
  await waitSim("run hello ok=true out=hello from the hub", 20000);
  console.log("PASS: phone ran a saved command on the Hub and got its output");
  // Phase 4: the phone watches the Hub's real screen (captured from the X display).
  await waitSim("screen frames=5", 30000);
  const m = simOut.match(/screen frames=5 size=(\d+)x(\d+) jpeg=(true|false)/);
  if (!m || m[3] !== "true") throw new Error(`bad frames: ${m?.[0]}`);
  console.log(`PASS: Hub screen streamed to the phone (${m[1]}×${m[2]} JPEG frames)`);
  // Phase 4: one keyboard and mouse — the phone stands in for a laptop to the right.
  await invoke("set_screen_layout", {
    layout: { enabled: true, links: [{ device: peer.info.id, edge: "right" }] },
  });
  const xdo = (args) =>
    new Promise((resolve) => spawn("xdotool", args, { stdio: "ignore" }).on("exit", resolve));
  await xdo(["mousemove", "700", "450"]);
  await sleep(300);
  await xdo(["mousemove", "1439", "450"]);
  await waitSim("remote input: move_to", 10000);
  await sleep(300);
  await xdo(["mousemove_relative", "--", "40", "10"]);
  await waitSim("remote input: move", 10000);
  const info = await invoke("screen_layout");
  if (!info.active) throw new Error("edge switching should be active");
  await xdo(["mousemove_relative", "--", "-80", "0"]);
  await sleep(600);
  if ((await invoke("screen_layout")).active) throw new Error("pointer should be back");
  await invoke("set_screen_layout", { layout: { enabled: false, links: [] } });
  console.log("PASS: pointer crossed the screen edge to the other device and came back");
  await click(`//nav//button[contains(., "Control")]`);
  await sleep(900);
  await shot("08b-control");
  await click(`//nav//button[contains(., "Alerts")]`);
  await sleep(800);
  await shot("08-alerts");
  await click(`//nav//button[contains(., "Files")]`);
  await sleep(800);
  await shot("09-files");
  await click(`//nav//button[contains(., "Battery")]`);
  await sleep(1200);
  await shot("10-battery");

  // Optional pause so performance can be measured on a live, connected dashboard.
  const hold = Number(process.env.DEVTI_SMOKE_HOLD_MS || 0);
  if (hold > 0) {
    console.log(`HOLDING ${hold} ms`);
    await sleep(hold);
  }

  // Kill switch over the real stack.
  await invoke("set_paused", { paused: true });
  await sleep(1500);
  const paused = await invoke("snapshot");
  if (paused.peers[0].online) throw new Error("peer still online while paused");
  await shot("11-paused");
  await invoke("set_paused", { paused: false });
  console.log("PASS: kill switch disconnects");
} catch (e) {
  failed = true;
  console.error("FAIL:", e.message);
  try {
    await shot("zz-failure");
  } catch {}
} finally {
  simProc?.kill();
  try {
    await wd("DELETE", s(""));
  } catch {}
  driver.kill();
  process.exit(failed ? 1 : 0);
}
