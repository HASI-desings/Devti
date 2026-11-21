import type {
  ActivityEntry,
  BatterySample,
  BluetoothEvent,
  Bridge,
  ClipboardEntry,
  DeviceInfo,
  DiscoveredDevice,
  LocalSnapshot,
  MirroredNotification,
  NodeEvent,
  PairingWindowInfo,
  PeerSnapshot,
  Permissions,
  Snapshot,
  TransferRecord,
} from "./types";

/**
 * Preview mode — used only when the UI runs in a plain browser (design review and the
 * Playwright UI tests). It simulates a small ecosystem so every screen can be
 * exercised. The UI shows a permanent "Preview" badge in this mode; the shipped app
 * always runs inside Tauri with the real core.
 */

const TRUSTED: Permissions = {
  screen_view: true,
  control: true,
  files: true,
  clipboard: true,
  notifications: true,
  commands: true,
  bluetooth: true,
};

const now = () => Date.now();

function randomDigits(n: number) {
  let s = "";
  for (let i = 0; i < n; i++) s += Math.floor(Math.random() * 10);
  return s;
}

function randomId() {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let s = "";
  for (let i = 0; i < 16; i++) s += alphabet[Math.floor(Math.random() * 32)];
  return s;
}

export interface PreviewControls {
  /** Simulates a phone scanning the QR code right now. */
  join(name?: string, kind?: DeviceInfo["kind"]): void;
  /** Simulates the joining phone tapping "Codes match". */
  remoteAccept(): void;
  /** Toggles a paired device online/offline. */
  setOnline(id: string, online: boolean): void;
  /** Simulates a paired device asking this computer to ring. */
  ringMe(on: boolean): void;
  reset(): void;
}

declare global {
  interface Window {
    __devtiPreview?: PreviewControls;
  }
}

export function createPreviewBridge(): Bridge {
  const listeners = new Set<(e: NodeEvent) => void>();
  const emit = (e: NodeEvent) => {
    for (const l of listeners) l(e);
  };

  let paused = false;
  const settings: Record<string, string> = {};
  try {
    Object.assign(settings, JSON.parse(localStorage.getItem("devti-preview-settings") ?? "{}"));
  } catch {
    // storage unavailable (private mode) — start fresh
  }
  const persist = () => {
    try {
      localStorage.setItem("devti-preview-settings", JSON.stringify(settings));
    } catch {
      // ignore
    }
  };

  let local: LocalSnapshot = {
    info: {
      id: "K7Q2M9XAB4C5D6E7",
      name: "Studio Laptop",
      kind: "laptop",
      platform: "linux",
      os_version: "Linux Mint 22",
      model: null,
      app_version: "0.1.0",
      capabilities: ["battery"],
    },
    fingerprint: "3f9a 11c2 8be0 77d4 0a5e 9c31 ffa2 4b81 9d0e 2c47 6a18 b3f5 0e9d 7c21 54aa 03be",
    port: 47810,
    addresses: ["192.168.1.24"],
    status: { battery: { percent: 76, charging: true } },
    paused: false,
    discovery_enabled: true,
    identity_origin: "created",
    secret_backend: "Secret Service (GNOME Keyring / KWallet)",
    secret_os_protected: true,
  };

  const peers = new Map<string, PeerSnapshot>();
  const nearby: DiscoveredDevice[] = [
    {
      id: "N2B7Q4XZ5K3M8P6R",
      name: "Office Desktop",
      kind: "desktop",
      platform: "windows",
      addrs: ["192.168.1.31:47810"],
      fingerprint: null,
      pairing_open: true,
    },
  ];
  const log: ActivityEntry[] = [];
  let logSeq = 1;
  const addLog = (action: string, device?: DeviceInfo, detail = "") => {
    log.unshift({
      id: logSeq++,
      ts: now(),
      device_id: device?.id ?? null,
      device_name: device?.name ?? null,
      action,
      detail,
    });
  };

  let window_: PairingWindowInfo | null = null;
  let dnd = false;
  let lowBattery = 15;
  const clipboard: ClipboardEntry[] = [];
  let clipSeq = 1;
  const transfers = new Map<number, TransferRecord>();
  let notifications: MirroredNotification[] = [];
  let pending: { id: number; peer: DeviceInfo; local: boolean; remote: boolean } | null = null;
  let joinTimer: ReturnType<typeof setTimeout> | undefined;

  const complete = () => {
    if (!pending?.local || !pending.remote) return;
    const peer: PeerSnapshot = {
      info: pending.peer,
      fingerprint: "c0de 77a1 9e3b 5f20 a4d8 61c9 0b7e 3a52 f1c6 8d09 2e4b 7a3f 95c1 06e8 d27b 4f10",
      role: "trusted",
      permissions: { ...TRUSTED },
      paired_at: now(),
      last_seen: now(),
      online: true,
      connection: { kind: "lan", address: "192.168.1.42:47810", rtt_ms: 6, since: now() },
      status: { battery: { percent: 64, charging: false } },
    };
    peers.set(peer.info.id, peer);
    addLog("paired", peer.info);
    addLog("connected", peer.info, peer.connection?.address ?? "");
    const id = pending.id;
    pending = null;
    window_ = null;
    emit({ type: "pairing_window_changed", window: null });
    emit({ type: "pairing_completed", pairing_id: id, peer });
    emit({ type: "peer_changed", peer });
    // The simulated phone shares a couple of notifications so the Notifications page
    // has something to show. Preview only.
    setTimeout(() => {
      const demo: MirroredNotification[] = [
        {
          from: peer.info.id,
          from_name: peer.info.name,
          notif: {
            key: `demo:${peer.info.id}:1`,
            app_id: "org.example.chat",
            app_name: "Messages",
            title: "Ayesha",
            text: "Are you coming to the studio today?",
            ts: now(),
            actions: [
              { id: "reply", label: "Reply", reply: true },
              { id: "read", label: "Mark as read", reply: false },
            ],
            icon_png: null,
            dismissable: true,
          },
        },
        {
          from: peer.info.id,
          from_name: peer.info.name,
          notif: {
            key: `demo:${peer.info.id}:2`,
            app_id: "org.example.calendar",
            app_name: "Calendar",
            title: "Design review",
            text: "Starts in 15 minutes",
            ts: now() - 60_000,
            actions: [],
            icon_png: null,
            dismissable: true,
          },
        },
      ];
      for (const n of demo) {
        notifications = [n, ...notifications.filter((x) => x.notif.key !== n.notif.key)];
        emit({ type: "notification_posted", notification: n, dnd });
      }
    }, 1200);
  };

  const controls: PreviewControls = {
    join(name = "Pixel 8", kind = "phone") {
      if (!window_) return;
      const peer: DeviceInfo = {
        id: randomId(),
        name,
        kind,
        platform: "android",
        os_version: "Android 15",
        model: name,
        app_version: "0.1.0",
        capabilities: ["battery"],
      };
      pending = { id: Math.floor(Math.random() * 1e9), peer, local: false, remote: false };
      emit({ type: "pairing_started", pairing_id: pending.id, role: "hub", peer });
      const sas = randomDigits(6);
      setTimeout(() => {
        if (!pending) return;
        emit({
          type: "pairing_verify",
          pairing_id: pending.id,
          role: "hub",
          peer,
          sas: `${sas.slice(0, 3)} ${sas.slice(3)}`,
        });
      }, 700);
    },
    remoteAccept() {
      if (!pending) return;
      pending.remote = true;
      complete();
    },
    setOnline(id, online) {
      const p = peers.get(id);
      if (!p) return;
      const next: PeerSnapshot = {
        ...p,
        online,
        last_seen: now(),
        connection: online ? { kind: "lan", address: "192.168.1.42:47810", rtt_ms: 7, since: now() } : null,
      };
      peers.set(id, next);
      addLog(online ? "connected" : "disconnected", p.info);
      emit({ type: "peer_changed", peer: next });
    },
    ringMe(on) {
      const p = [...peers.values()][0];
      emit({
        type: "ring_requested",
        from: p?.info.id ?? "demo",
        from_name: p?.info.name ?? "Demo phone",
        on,
      });
    },
    reset() {
      peers.clear();
      log.length = 0;
      for (const k of Object.keys(settings)) delete settings[k];
      persist();
    },
  };
  window.__devtiPreview = controls;

  // Ambient life: battery drains slowly on online preview peers.
  setInterval(() => {
    for (const p of peers.values()) {
      if (!p.online || !p.status.battery) continue;
      const b = p.status.battery;
      const percent = b.charging ? Math.min(100, b.percent + 1) : Math.max(4, b.percent - 1);
      const next = { ...p, status: { battery: { ...b, percent } } };
      peers.set(p.info.id, next);
      emit({ type: "peer_changed", peer: next });
    }
  }, 20_000);

  const snapshot = (): Snapshot => ({
    local,
    peers: [...peers.values()],
    nearby: [...nearby],
    window: window_,
    settings: { ...settings },
    dnd,
    low_battery_threshold: lowBattery,
    downloads_dir: "~/Downloads/Devti",
    notifications: [...notifications],
  });

  const pushTransfer = (t: TransferRecord) => {
    transfers.set(t.id, t);
    emit({ type: "transfer", transfer: t, rate_bps: t.state === "active" ? 18_000_000 : 0 });
  };

  return {
    mode: "preview",
    snapshot: async () => snapshot(),
    systemInfo: async () => ({
      os: "linux",
      os_version: "Linux Mint 22",
      session: "x11",
      autostart: settings.autostart === "true",
      app_version: "0.1.0",
      data_dir: "~/.local/share/app.devti.hub",
      log_dir: "~/.local/share/app.devti.hub/logs",
    }),
    onEvent(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    async openPairingWindow() {
      if (paused) throw new Error("The ecosystem is paused. Resume it to pair new devices.");
      const code = randomDigits(6);
      window_ = {
        id: Math.floor(Math.random() * 1e9),
        uri: `devti://pair?v=1&id=${local.info.id}&fp=preview&s=${randomId()}&p=47810&a=192.168.1.24&n=Studio%20Laptop&k=laptop`,
        code: `${code.slice(0, 3)} ${code.slice(3)}`,
        expires_at: now() + 5 * 60_000,
        addresses: ["192.168.1.24:47810"],
      };
      emit({ type: "pairing_window_changed", window: window_ });
      // Nothing joins on its own: the demo phone only appears when the person presses
      // "Simulate a phone" (see PairPanel), so preview never pretends to be real.
      clearTimeout(joinTimer);
      return window_;
    },
    async closePairingWindow() {
      clearTimeout(joinTimer);
      window_ = null;
      pending = null;
      emit({ type: "pairing_window_changed", window: null });
    },
    async confirmPairing(pairingId, accept) {
      if (!pending || pending.id !== pairingId) throw new Error("No pairing is waiting for confirmation.");
      if (!accept) {
        const id = pending.id;
        pending = null;
        emit({
          type: "pairing_failed",
          pairing_id: id,
          reason: "rejected",
          message: "Pairing was declined on one of the devices.",
        });
        return;
      }
      pending.local = true;
      // The simulated phone confirms a moment later.
      setTimeout(() => controls.remoteAccept(), 900);
    },
    async pairWithCode() {
      throw new Error("Pairing by code needs the real Devti app — preview mode can't reach other devices.");
    },
    async pairWithCodeAt() {
      throw new Error("Pairing by code needs the real Devti app — preview mode can't reach other devices.");
    },
    async revoke(id) {
      const p = peers.get(id);
      if (!p) throw new Error("That device is not paired.");
      peers.delete(id);
      addLog("revoked", p.info);
      emit({ type: "peer_removed", id, reason: "revoked_here" });
    },
    async setAccess(id, role, permissions) {
      const p = peers.get(id);
      if (!p) throw new Error("That device is not paired.");
      const next = { ...p, role, permissions };
      peers.set(id, next);
      addLog("permissions_changed", p.info, role);
      emit({ type: "peer_changed", peer: next });
      return next;
    },
    async setPaused(next) {
      paused = next;
      local = { ...local, paused: next };
      if (next) {
        window_ = null;
        emit({ type: "pairing_window_changed", window: null });
        for (const p of peers.values()) {
          const off = { ...p, online: false, connection: null, last_seen: now() };
          peers.set(p.info.id, off);
          emit({ type: "peer_changed", peer: off });
        }
      }
      addLog(next ? "ecosystem_paused" : "ecosystem_resumed");
      emit({ type: "paused", paused: next });
      emit({ type: "local_changed", local });
    },
    async rename(name) {
      local = { ...local, info: { ...local.info, name: name.trim().slice(0, 48) || "Unnamed device" } };
      emit({ type: "local_changed", local });
      return local;
    },
    async activity(limit, beforeId, deviceId) {
      return log
        .filter((e) => (beforeId ? e.id < beforeId : true) && (deviceId ? e.device_id === deviceId : true))
        .slice(0, limit);
    },
    async setSetting(key, value) {
      settings[key] = value;
      persist();
    },
    async setAutostart(enabled) {
      settings.autostart = String(enabled);
      persist();
      return enabled;
    },
    async bluetoothScan(cb: (e: BluetoothEvent) => void) {
      cb({ type: "adapter", available: true, message: "Scanning" });
      const found = [
        { id: "bt-1", name: "WF-1000XM5", rssi: -48, services: ["Battery"], manufacturer: "Sony" },
        { id: "bt-2", name: "Galaxy Buds3 Pro", rssi: -61, services: ["Battery"], manufacturer: "Samsung" },
        { id: "bt-3", name: null, rssi: -84, services: [], manufacturer: "Apple" },
      ];
      const timers = found.map((d, i) => setTimeout(() => cb({ type: "device", device: d }), 900 + i * 1300));
      return () => {
        for (const t of timers) clearTimeout(t);
        cb({ type: "stopped" });
      };
    },
    async openExternal(url) {
      window.open(url, "_blank", "noopener");
    },
    async exportDiagnostics() {
      return "Preview mode has no diagnostics to export.";
    },
    onUiAction() {
      return () => {};
    },
    async takePendingSend() {
      return [];
    },
    async clipboardHistory() {
      return [...clipboard];
    },
    async clearClipboardHistory() {
      clipboard.length = 0;
    },
    async copyToClipboard(text) {
      await navigator.clipboard?.writeText(text).catch(() => {});
    },
    async shareClipboard(text) {
      clipboard.unshift({ id: clipSeq++, ts: now(), device_id: null, device_name: null, text });
      return true;
    },
    async ring(id, on) {
      const p = peers.get(id);
      if (!p?.online) throw new Error("That device is offline.");
      addLog(on ? "ring_sent" : "ring_stop_sent", p.info);
    },
    async notificationAction(id, key, action, reply) {
      const n = notifications.find((x) => x.from === id && x.notif.key === key);
      if (!n) throw new Error("That notification is gone.");
      addLog("notification_action", peers.get(id)?.info, reply ? `${action}: ${reply}` : action);
    },
    async dismissNotification(id, key) {
      notifications = notifications.filter((x) => !(x.from === id && x.notif.key === key));
      emit({ type: "notification_removed", from: id, key });
    },
    async setDnd(on) {
      dnd = on;
      emit({ type: "dnd_changed", on, by: null });
    },
    async pickFiles() {
      // Browsers don't reveal file paths; preview simulates the transfer by name only.
      return new Promise<string[]>((resolve) => {
        const input = document.createElement("input");
        input.type = "file";
        input.multiple = true;
        input.onchange = () => resolve([...(input.files ?? [])].map((f) => `preview:${f.name}:${f.size}`));
        input.click();
      });
    },
    async sendFiles(id, paths) {
      const p = peers.get(id);
      if (!p) throw new Error("That device is not paired.");
      return paths.map((path) => {
        const [, name = "file", sizeText = "4000000"] = path.split(":");
        const size = Number(sizeText) || 4_000_000;
        const t: TransferRecord = {
          id: Math.floor(Math.random() * 1e12),
          device_id: id,
          device_name: p.info.name,
          incoming: false,
          name,
          path,
          size,
          done: 0,
          state: p.online ? "active" : "queued",
          ts: now(),
        };
        pushTransfer(t);
        if (p.online) {
          let step = 0;
          const timer = setInterval(() => {
            step++;
            const cur = transfers.get(t.id);
            if (cur?.state !== "active") return clearInterval(timer);
            const done = Math.min(size, Math.round((size * step) / 12));
            pushTransfer({ ...cur, done, state: done >= size ? "done" : "active" });
            if (done >= size) clearInterval(timer);
          }, 250);
        }
        return t.id;
      });
    },
    async transfers() {
      return [...transfers.values()].sort((a, b) => b.ts - a.ts);
    },
    async cancelTransfer(id) {
      const t = transfers.get(id);
      if (t) pushTransfer({ ...t, state: "cancelled" });
    },
    async retryTransfer(id) {
      const t = transfers.get(id);
      if (t) pushTransfer({ ...t, state: "queued" });
    },
    async clearFinishedTransfers() {
      for (const [id, t] of transfers)
        if (["done", "failed", "cancelled"].includes(t.state)) transfers.delete(id);
    },
    async openPath() {
      throw new Error("Opening files needs the Devti desktop app.");
    },
    async revealPath() {
      throw new Error("Opening folders needs the Devti desktop app.");
    },
    async chooseDownloadsDir() {
      return null;
    },
    async batteryHistory(id, since): Promise<BatterySample[]> {
      // A plausible, continuous simulation: ~4 %/h discharge, charge at ~45 %/h
      // when it falls below 25 %, stop at 100 %. Preview only.
      const out: BatterySample[] = [];
      const step = 20 * 60_000;
      const end = now();
      const start = Math.max(since, end - 7 * 24 * 3600_000);
      let level = 60 + ((id ?? "self").charCodeAt(0) % 30);
      let charging = false;
      for (let t = start; t <= end; t += step) {
        if (charging) {
          level = Math.min(100, level + 15);
          if (level >= 100) charging = false;
        } else {
          level = Math.max(3, level - 1.35);
          if (level < 25) charging = true;
        }
        out.push({ ts: t, percent: Math.round(level), charging });
      }
      return out;
    },
    async setLowBatteryThreshold(p) {
      lowBattery = p;
    },
    onFileDrop() {
      return () => {};
    },
    async takeUiAction() {
      return null;
    },
  };
}
