// TypeScript mirror of the JSON the Rust core emits (core/src/node/events.rs).
// Keep in sync with docs/PROTOCOL.md § Host API.

export type DeviceKind = "desktop" | "laptop" | "phone" | "tablet" | "accessory" | "unknown";
export type Platform = "windows" | "linux" | "macos" | "android" | "ios" | "ipados" | "other";
export type TrustRole = "trusted" | "restricted";

export interface DeviceInfo {
  id: string;
  name: string;
  kind: DeviceKind;
  platform: Platform;
  os_version: string;
  model: string | null;
  app_version: string;
  capabilities: string[];
}

export interface BatteryState {
  percent: number;
  charging: boolean;
}

export interface DeviceStatus {
  battery: BatteryState | null;
}

export interface Permissions {
  screen_view: boolean;
  control: boolean;
  files: boolean;
  clipboard: boolean;
  notifications: boolean;
  commands: boolean;
  bluetooth: boolean;
}

export interface ConnectionSnapshot {
  kind: "lan";
  address: string;
  rtt_ms: number;
  since: number;
}

export interface PeerSnapshot {
  info: DeviceInfo;
  fingerprint: string;
  role: TrustRole;
  permissions: Permissions;
  paired_at: number;
  last_seen: number | null;
  online: boolean;
  connection: ConnectionSnapshot | null;
  status: DeviceStatus;
}

export interface LocalSnapshot {
  info: DeviceInfo;
  fingerprint: string;
  port: number;
  addresses: string[];
  status: DeviceStatus;
  paused: boolean;
  discovery_enabled: boolean;
  identity_origin: "created" | "loaded" | "regenerated";
  secret_backend: string;
  secret_os_protected: boolean;
}

export interface DiscoveredDevice {
  id: string;
  name: string;
  kind: DeviceKind;
  platform: Platform;
  addrs: string[];
  fingerprint: string | null;
  pairing_open: boolean;
}

export interface PairingWindowInfo {
  id: number;
  uri: string;
  code: string;
  expires_at: number;
  addresses: string[];
}

export type PairAbortReason =
  | "bad_secret"
  | "rejected"
  | "not_accepting"
  | "busy"
  | "incompatible"
  | "identity_mismatch"
  | "timeout"
  | "other";

export type NodeEvent =
  | { type: "local_changed"; local: LocalSnapshot }
  | { type: "peer_changed"; peer: PeerSnapshot }
  | { type: "peer_removed"; id: string; reason: "revoked_here" | "revoked_by_peer" }
  | { type: "device_discovered"; device: DiscoveredDevice }
  | { type: "device_lost"; id: string }
  | { type: "pairing_window_changed"; window: PairingWindowInfo | null }
  | { type: "pairing_started"; pairing_id: number; role: "hub" | "joiner"; peer: DeviceInfo | null }
  | { type: "pairing_verify"; pairing_id: number; role: "hub" | "joiner"; peer: DeviceInfo; sas: string }
  | { type: "pairing_completed"; pairing_id: number; peer: PeerSnapshot }
  | { type: "pairing_failed"; pairing_id: number; reason: PairAbortReason; message: string }
  | { type: "paused"; paused: boolean }
  /** Events were dropped (the UI fell behind); reload the full snapshot. */
  | { type: "resync" }
  | { type: "clipboard_received"; from: string; from_name: string; text: string; ts: number }
  | { type: "ring_requested"; from: string; from_name: string; on: boolean }
  | { type: "notification_posted"; notification: MirroredNotification; dnd: boolean }
  | { type: "notification_removed"; from: string; key: string }
  | { type: "notification_action_requested"; from: string; key: string; action: string; reply: string | null }
  | { type: "notification_dismiss_requested"; from: string; key: string }
  | { type: "dnd_changed"; on: boolean; by: string | null }
  | { type: "low_battery"; id: string; name: string; percent: number }
  | { type: "transfer"; transfer: TransferRecord; rate_bps: number };

export interface NotifAction {
  id: string;
  label: string;
  reply: boolean;
}

export interface NotificationInfo {
  key: string;
  app_id: string;
  app_name: string;
  title: string;
  text: string;
  ts: number;
  actions: NotifAction[];
  icon_png: number[] | null;
  dismissable: boolean;
}

export interface MirroredNotification {
  from: string;
  from_name: string;
  notif: NotificationInfo;
}

export type TransferState = "queued" | "active" | "paused" | "done" | "failed" | "cancelled";

export interface TransferRecord {
  id: number;
  device_id: string;
  device_name: string;
  incoming: boolean;
  name: string;
  path: string;
  size: number;
  done: number;
  state: TransferState;
  ts: number;
}

export interface ClipboardEntry {
  id: number;
  ts: number;
  device_id: string | null;
  device_name: string | null;
  text: string;
}

export interface BatterySample {
  ts: number;
  percent: number;
  charging: boolean;
}

export interface ActivityEntry {
  id: number;
  ts: number;
  device_id: string | null;
  device_name: string | null;
  action: string;
  detail: string;
}

export interface Snapshot {
  local: LocalSnapshot;
  peers: PeerSnapshot[];
  nearby: DiscoveredDevice[];
  window: PairingWindowInfo | null;
  settings: Record<string, string>;
  dnd: boolean;
  low_battery_threshold: number;
  downloads_dir: string;
  notifications: MirroredNotification[];
}

export interface BluetoothDevice {
  id: string;
  name: string | null;
  rssi: number | null;
  /** Human labels for advertised standard services ("Battery", "Heart rate"…). */
  services: string[];
  manufacturer: string | null;
}

export type BluetoothEvent =
  | { type: "adapter"; available: boolean; message: string }
  | { type: "device"; device: BluetoothDevice }
  | { type: "stopped" };

export interface SystemInfo {
  os: Platform;
  os_version: string;
  /** "wayland" | "x11" | "windows" | "unknown" */
  session: string;
  autostart: boolean;
  app_version: string;
  data_dir: string;
  log_dir: string;
}

/** Everything the UI may ask of the backend. Implemented by Tauri and by preview mode. */
export interface Bridge {
  readonly mode: "tauri" | "preview";
  snapshot(): Promise<Snapshot>;
  systemInfo(): Promise<SystemInfo>;
  onEvent(cb: (e: NodeEvent) => void): () => void;
  openPairingWindow(): Promise<PairingWindowInfo>;
  closePairingWindow(): Promise<void>;
  confirmPairing(pairingId: number, accept: boolean): Promise<void>;
  pairWithCode(deviceId: string, code: string): Promise<number>;
  pairWithCodeAt(address: string, code: string): Promise<number>;
  revoke(id: string): Promise<void>;
  setAccess(id: string, role: TrustRole, permissions: Permissions): Promise<PeerSnapshot>;
  setPaused(paused: boolean): Promise<void>;
  rename(name: string): Promise<LocalSnapshot>;
  activity(limit: number, beforeId?: number, deviceId?: string): Promise<ActivityEntry[]>;
  setSetting(key: string, value: string): Promise<void>;
  setAutostart(enabled: boolean): Promise<boolean>;
  bluetoothScan(cb: (e: BluetoothEvent) => void): Promise<() => void>;
  openExternal(url: string): Promise<void>;
  exportDiagnostics(): Promise<string>;
  /** Actions requested from outside the window (tray menu), e.g. "pair". */
  onUiAction(cb: (action: string) => void): () => void;
  takeUiAction(): Promise<string | null>;
  // Phase 2
  takePendingSend(): Promise<string[]>;
  clipboardHistory(): Promise<ClipboardEntry[]>;
  clearClipboardHistory(): Promise<void>;
  copyToClipboard(text: string): Promise<void>;
  shareClipboard(text: string): Promise<boolean>;
  ring(id: string, on: boolean): Promise<void>;
  notificationAction(id: string, key: string, action: string, reply: string | null): Promise<void>;
  dismissNotification(id: string, key: string): Promise<void>;
  setDnd(on: boolean): Promise<void>;
  pickFiles(): Promise<string[]>;
  sendFiles(id: string, paths: string[]): Promise<number[]>;
  transfers(): Promise<TransferRecord[]>;
  cancelTransfer(id: number): Promise<void>;
  retryTransfer(id: number): Promise<void>;
  clearFinishedTransfers(): Promise<void>;
  openPath(path: string): Promise<void>;
  revealPath(path: string): Promise<void>;
  chooseDownloadsDir(): Promise<string | null>;
  batteryHistory(id: string | null, since: number): Promise<BatterySample[]>;
  setLowBatteryThreshold(percent: number): Promise<void>;
  /** Native drag and drop of files onto the window (paths + drop point). */
  onFileDrop(cb: (paths: string[], x: number, y: number) => void): () => void;
}
