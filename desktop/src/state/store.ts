import { create } from "zustand";
import type {
  Bridge,
  CallInfo,
  Catalog,
  DeviceInfo,
  DiscoveredDevice,
  LocalSnapshot,
  MirroredNotification,
  NodeEvent,
  PairAbortReason,
  PairingWindowInfo,
  PeerSnapshot,
  Snapshot,
  TransferRecord,
} from "../bridge";
import { assignVoices, type Voice } from "../design/voice";

export interface FsListing {
  entries: import("../bridge").FsEntry[];
  truncated: boolean;
  error: string | null;
  at: number;
}

export type Page =
  | "ecosystem"
  | "device"
  | "files"
  | "notifications"
  | "battery"
  | "security"
  | "settings"
  | "help"
  | "control"
  | "mirror"
  | "screens"
  | "accessories"
  | "messages"
  | "automation"
  | "plugins";

export type PairingStage =
  | { stage: "waiting" }
  | { stage: "connecting"; pairingId: number; role: "hub" | "joiner"; peer: DeviceInfo | null }
  | {
      stage: "verify";
      pairingId: number;
      role: "hub" | "joiner";
      peer: DeviceInfo;
      sas: string;
      confirmed: boolean;
    }
  | { stage: "done"; peer: PeerSnapshot }
  | { stage: "failed"; reason: PairAbortReason; message: string };

export interface Toast {
  id: number;
  text: string;
  tone: "info" | "ok" | "danger";
}

interface State {
  ready: boolean;
  error: string | null;
  mode: "tauri" | "preview";
  local: LocalSnapshot | null;
  peers: Record<string, PeerSnapshot>;
  nearby: Record<string, DiscoveredDevice>;
  window: PairingWindowInfo | null;
  settings: Record<string, string>;
  voices: Record<string, Voice>;
  /** Devices that came online in the last few seconds (drives the "linking" animation). */
  justLinked: Record<string, number>;
  pairing: PairingStage | null;
  page: Page;
  deviceId: string | null;
  selected: string | null;
  toasts: Toast[];
  tour: boolean;
  dnd: boolean;
  lowBatteryThreshold: number;
  downloadsDir: string;
  notifications: MirroredNotification[];
  transfers: Record<number, TransferRecord & { rate: number }>;
  /** A device asked this computer to ring (find my device). */
  ringing: { from: string; name: string } | null;
  /** Files waiting for the user to pick a device (drag and drop, Send to Devti). */
  sendPaths: string[] | null;
  /** What other devices offer (apps, saved commands, deck, system actions). */
  catalogs: Record<string, Catalog>;
  /** Latest results of commands this computer ran on other devices. */
  runResults: { id: number; from: string; command: string; ok: boolean; output: string; ts: number }[];
  /** Live screen stats per device (viewing or sharing). */
  screenStats: Record<string, { sharing: boolean; fps: number; latency_ms: number; kbps: number }>;
  /** Devices watching this computer's screen right now. */
  viewers: Record<string, string>;
  /** Phone conversations, by phone id. */
  smsThreads: Record<string, import("../bridge").SmsThread[]>;
  /** Messages, by `${phone}/${thread}`. */
  smsMessages: Record<string, import("../bridge").SmsMessage[]>;
  /** Shared-folder listings by `${device}|${path}` (Phase 7). */
  fsListings: Record<string, FsListing>;
  /** The current or last call per phone. */
  calls: Record<string, CallInfo & { from_name: string }>;
}

interface Actions {
  load(snapshot: Snapshot, mode: "tauri" | "preview"): void;
  fail(error: string): void;
  apply(event: NodeEvent): void;
  navigate(page: Page, deviceId?: string): void;
  select(id: string | null): void;
  setSetting(key: string, value: string): void;
  setPairing(p: PairingStage | null): void;
  toast(text: string, tone?: Toast["tone"]): void;
  dismissToast(id: number): void;
  setTour(on: boolean): void;
  setTransfers(list: TransferRecord[]): void;
  setSendPaths(paths: string[] | null): void;
  stopRinging(): void;
}

let toastSeq = 1;

function withVoices(peers: Record<string, PeerSnapshot>) {
  const ordered = Object.values(peers)
    .sort((a, b) => a.paired_at - b.paired_at)
    .map((p) => ({ id: p.info.id }));
  return assignVoices(ordered);
}

export const useStore = create<State & Actions>((set, get) => ({
  ready: false,
  error: null,
  mode: "preview",
  local: null,
  peers: {},
  nearby: {},
  window: null,
  settings: {},
  voices: {},
  justLinked: {},
  pairing: null,
  page: "ecosystem",
  deviceId: null,
  selected: null,
  toasts: [],
  tour: false,
  dnd: false,
  lowBatteryThreshold: 15,
  downloadsDir: "",
  notifications: [],
  transfers: {},
  ringing: null,
  sendPaths: null,
  catalogs: {},
  runResults: [],
  screenStats: {},
  viewers: {},
  smsThreads: {},
  smsMessages: {},
  fsListings: {},
  calls: {},

  load(s, mode) {
    const peers = Object.fromEntries(s.peers.map((p) => [p.info.id, p]));
    set({
      ready: true,
      error: null,
      mode,
      local: s.local,
      peers,
      nearby: Object.fromEntries(s.nearby.map((d) => [d.id, d])),
      window: s.window,
      settings: s.settings,
      voices: withVoices(peers),
      dnd: s.dnd ?? false,
      lowBatteryThreshold: s.low_battery_threshold ?? 15,
      downloadsDir: s.downloads_dir ?? "",
      notifications: s.notifications ?? [],
    });
  },

  fail(error) {
    set({ error, ready: false });
  },

  apply(e) {
    const st = get();
    switch (e.type) {
      case "local_changed":
        set({ local: e.local });
        break;
      case "peer_changed": {
        const prev = st.peers[e.peer.info.id];
        const peers = { ...st.peers, [e.peer.info.id]: e.peer };
        const justLinked = { ...st.justLinked };
        if (e.peer.online && !prev?.online) justLinked[e.peer.info.id] = Date.now();
        if (!e.peer.online) delete justLinked[e.peer.info.id];
        const nearby = { ...st.nearby };
        delete nearby[e.peer.info.id];
        set({ peers, voices: prev ? st.voices : withVoices(peers), justLinked, nearby });
        break;
      }
      case "peer_removed": {
        const peers = { ...st.peers };
        const gone = peers[e.id];
        delete peers[e.id];
        set({
          peers,
          voices: withVoices(peers),
          selected: st.selected === e.id ? null : st.selected,
          ...(st.deviceId === e.id ? { page: "ecosystem" as Page, deviceId: null } : {}),
        });
        if (gone && e.reason === "revoked_by_peer") {
          get().toast(`${gone.info.name} removed this computer from its devices.`, "info");
        }
        break;
      }
      case "device_discovered":
        if (!st.peers[e.device.id]) set({ nearby: { ...st.nearby, [e.device.id]: e.device } });
        break;
      case "device_lost": {
        const nearby = { ...st.nearby };
        delete nearby[e.id];
        set({ nearby });
        break;
      }
      case "pairing_window_changed":
        set({ window: e.window });
        if (e.window && (!st.pairing || st.pairing.stage === "failed" || st.pairing.stage === "done")) {
          set({ pairing: { stage: "waiting" } });
        }
        break;
      case "pairing_started":
        set({ pairing: { stage: "connecting", pairingId: e.pairing_id, role: e.role, peer: e.peer } });
        break;
      case "pairing_verify":
        set({
          pairing: {
            stage: "verify",
            pairingId: e.pairing_id,
            role: e.role,
            peer: e.peer,
            sas: e.sas,
            confirmed: false,
          },
        });
        break;
      case "pairing_completed":
        set({ pairing: { stage: "done", peer: e.peer } });
        break;
      case "pairing_failed":
        set({ pairing: { stage: "failed", reason: e.reason, message: e.message } });
        break;
      case "paused":
        if (st.local) set({ local: { ...st.local, paused: e.paused } });
        break;
      case "resync":
        break;
      case "notification_posted": {
        const n = e.notification;
        const rest = st.notifications.filter((x) => !(x.from === n.from && x.notif.key === n.notif.key));
        set({ notifications: [n, ...rest].slice(0, 300) });
        break;
      }
      case "notification_removed":
        set({ notifications: st.notifications.filter((x) => !(x.from === e.from && x.notif.key === e.key)) });
        break;
      case "dnd_changed":
        set({ dnd: e.on });
        break;
      case "ring_requested":
        set({ ringing: e.on ? { from: e.from, name: e.from_name } : null });
        break;
      case "low_battery":
        get().toast(`${e.name} is at ${e.percent}%. Plug it in soon.`, "danger");
        break;
      case "clipboard_received":
        break;
      case "transfer": {
        const t = e.transfer;
        set({ transfers: { ...st.transfers, [t.id]: { ...t, rate: e.rate_bps } } });
        if (t.state === "done" && st.transfers[t.id]?.state !== "done") {
          get().toast(
            t.incoming ? `Received ${t.name} from ${t.device_name}.` : `Sent ${t.name} to ${t.device_name}.`,
            "ok",
          );
        }
        if (t.state === "failed" && st.transfers[t.id]?.state !== "failed") {
          get().toast(`${t.name} couldn't be sent to ${t.device_name}.`, "danger");
        }
        break;
      }
      case "notification_action_requested":
      case "notification_dismiss_requested":
        break;
      case "catalog_received":
        set({ catalogs: { ...st.catalogs, [e.from]: e.catalog } });
        break;
      case "run_finished": {
        const r = {
          id: toastSeq++,
          from: e.from,
          command: e.command,
          ok: e.ok,
          output: e.output,
          ts: Date.now(),
        };
        set({ runResults: [r, ...st.runResults].slice(0, 30) });
        break;
      }
      case "screen_stats":
        set({ screenStats: { ...st.screenStats, [e.peer]: e } });
        break;
      case "screen_ended": {
        const screenStats = { ...st.screenStats };
        delete screenStats[e.peer];
        set({ screenStats });
        if (st.page === "mirror" && st.deviceId === e.peer) get().toast("The screen stopped.", "info");
        break;
      }
      case "screen_requested": {
        const viewers = { ...st.viewers };
        if (e.on) viewers[e.from] = e.from_name;
        else delete viewers[e.from];
        set({ viewers });
        break;
      }
      case "sms_threads":
        set({ smsThreads: { ...st.smsThreads, [e.from]: e.threads } });
        break;
      case "sms_messages":
        set({ smsMessages: { ...st.smsMessages, [`${e.from}/${e.thread}`]: e.messages } });
        break;
      case "sms": {
        const key = `${e.from}/${e.message.thread}`;
        const list = (st.smsMessages[key] ?? []).filter((m) => m.id !== e.message.id);
        const threads = st.smsThreads[e.from] ?? [];
        const known = threads.find((t) => t.id === e.message.thread);
        const thread = {
          id: e.message.thread,
          addresses: known?.addresses ?? [e.message.address],
          names: known?.names ?? (e.message.name ? [e.message.name] : []),
          snippet: e.message.body,
          ts: e.message.ts,
          unread: e.message.outgoing ? 0 : (known?.unread ?? 0) + 1,
        };
        set({
          smsMessages: { ...st.smsMessages, [key]: [...list, e.message] },
          smsThreads: { ...st.smsThreads, [e.from]: [thread, ...threads.filter((t) => t.id !== thread.id)] },
        });
        break;
      }
      case "sms_result":
        if (!e.ok) get().toast(e.error ?? "The text wasn't sent.", "danger");
        break;
      case "call":
        set({ calls: { ...st.calls, [e.from]: { ...e.call, from_name: e.from_name } } });
        break;
      case "fs_listing":
        set({
          fsListings: {
            ...st.fsListings,
            [`${e.from}|${e.path}`]: {
              entries: e.entries,
              truncated: e.truncated,
              error: e.error,
              at: Date.now(),
            },
          },
        });
        break;
      case "fs_error":
        get().toast(`${e.path.split("/").pop() ?? "That file"}: ${e.error}`, "danger");
        break;
      case "reach_changed":
        break;
      case "handoff":
        get().toast(`Continued from ${e.from_name}${e.item.title ? `: ${e.item.title}` : ""}.`, "info");
        break;
      case "camera_offered":
      case "audio_started":
      case "audio_ended":
      case "sms_threads_requested":
      case "sms_messages_requested":
      case "sms_send_requested":
      case "call_action_requested":
        break;
      case "system_requested":
      case "launch_requested":
      case "run_requested":
      case "deck_pressed":
      case "remote_input":
        break;
    }
  },

  navigate(page, deviceId) {
    set({ page, deviceId: deviceId ?? null });
  },
  select(id) {
    set({ selected: id });
  },
  setSetting(key, value) {
    set({ settings: { ...get().settings, [key]: value } });
  },
  setPairing(p) {
    set({ pairing: p });
  },
  toast(text, tone = "info") {
    const id = toastSeq++;
    set({ toasts: [...get().toasts, { id, text, tone }] });
    setTimeout(() => get().dismissToast(id), 5200);
  },
  dismissToast(id) {
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  },
  setTour(on) {
    set({ tour: on });
  },
  setTransfers(list) {
    set({ transfers: Object.fromEntries(list.map((t) => [t.id, { ...t, rate: 0 }])) });
  },
  setSendPaths(paths) {
    set({ sendPaths: paths?.length ? paths : null });
  },
  stopRinging() {
    set({ ringing: null });
  },
}));

/** Paired devices ordered for display: online first, then by pairing time. */
export function orderedPeers(peers: Record<string, PeerSnapshot>): PeerSnapshot[] {
  return Object.values(peers).sort((a, b) =>
    a.online === b.online ? a.paired_at - b.paired_at : a.online ? -1 : 1,
  );
}

export type { Bridge };
