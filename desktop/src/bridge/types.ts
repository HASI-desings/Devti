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
  /** Browse and download from this device's shared folders (Phase 7). */
  browse: boolean;
}

export interface ConnectionSnapshot {
  /**
   * How the session runs: local network, a private VPN (Tailscale…), directly over the
   * internet (a hole punched through both routers, or a forwarded port), or through an
   * encrypted relay.
   */
  kind: "lan" | "vpn" | "internet" | "relay";
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
  | { type: "transfer"; transfer: TransferRecord; rate_bps: number }
  | { type: "remote_input"; from: string; events: InputEvent[] }
  | { type: "system_requested"; from: string; from_name: string; action: SystemAction }
  | { type: "launch_requested"; from: string; from_name: string; app: string }
  | { type: "run_requested"; from: string; from_name: string; command: string }
  | { type: "run_finished"; from: string; command: string; ok: boolean; output: string }
  | { type: "deck_pressed"; from: string; from_name: string; button: string }
  | { type: "catalog_received"; from: string; catalog: Catalog }
  | { type: "screen_requested"; from: string; from_name: string; on: boolean; source?: string }
  | { type: "screen_stats"; peer: string; sharing: boolean; fps: number; latency_ms: number; kbps: number }
  | { type: "screen_ended"; peer: string }
  | { type: "camera_offered"; from: string; from_name: string }
  | { type: "sms_threads"; from: string; threads: SmsThread[] }
  | { type: "sms_messages"; from: string; thread: string; messages: SmsMessage[] }
  | { type: "sms"; from: string; from_name: string; message: SmsMessage }
  | { type: "sms_result"; from: string; ok: boolean; error: string | null }
  | { type: "sms_threads_requested"; from: string }
  | { type: "sms_messages_requested"; from: string; thread: string }
  | { type: "sms_send_requested"; from: string; to: string[]; text: string }
  | { type: "call"; from: string; from_name: string; call: CallInfo }
  | { type: "call_action_requested"; from: string; id: string; action: string }
  | { type: "handoff"; from: string; from_name: string; item: HandoffItem }
  | {
      type: "audio_started";
      from: string;
      from_name: string;
      kind: string;
      sample_rate: number;
      channels: number;
    }
  | { type: "audio_ended"; from: string; kind: string }
  | { type: "reach_changed"; peer: string }
  | { type: "anywhere_changed" }
  | {
      type: "fs_listing";
      from: string;
      path: string;
      entries: FsEntry[];
      truncated: boolean;
      error: string | null;
    }
  | { type: "fs_error"; from: string; path: string; error: string };

// ------------------------------------------------------------------ Phase 7

/** Connecting from anywhere (built in since 1.1). */
export interface AnywhereInfo {
  enabled: boolean;
  /** The endpoint is running. */
  running: boolean;
  /** Connected to a relay, so devices can find this one from anywhere. */
  online: boolean;
  /** Host name of the relay this device is reachable through. */
  relay: string | null;
  /** The user's own relay URL ("" = the free public relays). */
  custom_relay: string;
  /** This network's public address as seen from the internet. */
  public_ip: string | null;
  error: string | null;
}

export interface ReachAddress {
  address: string;
  kind: "lan" | "vpn" | "internet" | "manual";
  interface: string;
}

export interface ReachInfo {
  enabled: boolean;
  port: number;
  addresses: ReachAddress[];
  extra: string[];
}

export interface SharedFolder {
  name: string;
  path: string;
}

export interface FsEntry {
  name: string;
  dir: boolean;
  size: number;
  modified: number;
}

export interface PluginCommand {
  id: string;
  name: string;
}

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  permissions: string[];
  commands: PluginCommand[];
}

export interface PluginInfo {
  manifest: PluginManifest;
  enabled: boolean;
  running: boolean;
  needs_approval: boolean;
  /** [permission, what it lets the plugin do] */
  permissions: [string, string][];
  problem: string | null;
  log: string[];
}

// ------------------------------------------------------------------ Phase 6

export interface SmsThread {
  id: string;
  addresses: string[];
  names: string[];
  snippet: string;
  ts: number;
  unread: number;
}

export interface SmsMessage {
  id: string;
  thread: string;
  address: string;
  name: string;
  body: string;
  ts: number;
  outgoing: boolean;
}

export interface CallInfo {
  id: string;
  state: "ringing" | "active" | "ended" | "missed";
  number: string;
  name: string;
  incoming: boolean;
  ts: number;
}

export interface HandoffItem {
  kind: "url" | "text" | "media";
  url: string;
  title: string;
  text: string;
  position_ms: number | null;
}

export type RuleTrigger =
  | { kind: "device_connected"; device: string }
  | { kind: "device_disconnected"; device: string }
  | { kind: "battery_below"; device: string; percent: number }
  | { kind: "accessory_connected"; name: string }
  | { kind: "proximity_away"; device: string }
  | { kind: "proximity_near"; device: string }
  | { kind: "time"; hour: number; minute: number; days: number[] }
  | { kind: "app_opened"; app: string }
  | { kind: "network_changed"; network: string };

export type RuleAction =
  | { kind: "notify"; title: string; body: string }
  | { kind: "lock_computer" }
  | { kind: "set_dnd"; on: boolean }
  | { kind: "run_command"; command: string }
  | { kind: "launch_app"; app: string }
  | { kind: "switch_audio_output"; output: string }
  | { kind: "accessory_control"; accessory: string; control: string; value: string }
  | { kind: "ring_device"; device: string };

export interface Rule {
  id: string;
  name: string;
  enabled: boolean;
  trigger: RuleTrigger;
  actions: RuleAction[];
  cooldown_s: number;
}

export interface AutomationInfo {
  rules: Rule[];
  log: { rule: string; name: string; ts: number; ok: boolean; detail: string }[];
  proximity: Record<string, boolean>;
}

export interface MediaStatus {
  audio: [string, string][];
  cameras: string[];
  webcam: string;
  mic: string;
}

// ------------------------------------------------------------------ Phase 3

export type MouseButton = "left" | "right" | "middle";

/** Input for another device to perform. Coordinates in move_to/tap/swipe are 0..1. */
export type InputEvent =
  | { k: "move"; dx: number; dy: number }
  | { k: "move_to"; x: number; y: number }
  | { k: "button"; button: MouseButton; down: boolean }
  | { k: "click"; button: MouseButton; count: number }
  | { k: "scroll"; dx: number; dy: number }
  | { k: "text"; text: string }
  | { k: "key"; key: string; mods: string[] }
  | { k: "media"; action: string }
  | { k: "tap"; x: number; y: number }
  | { k: "long_press"; x: number; y: number }
  | { k: "swipe"; x1: number; y1: number; x2: number; y2: number; ms: number }
  | { k: "nav"; action: string };

export type SystemAction =
  | { a: "lock" }
  | { a: "sleep" }
  | { a: "shutdown" }
  | { a: "restart" }
  | { a: "sign_out" }
  | { a: "set_volume"; percent: number }
  | { a: "set_brightness"; percent: number };

export interface Catalog {
  apps: { id: string; name: string }[];
  commands: { id: string; name: string; confirm: boolean }[];
  deck: { name: string; buttons: { id: string; label: string; icon: string; voice: string }[] }[];
  system: string[];
}

export interface SavedCommand {
  id: string;
  name: string;
  command: string;
  confirm: boolean;
}

export type DeckActionKind = "hotkey" | "launch" | "command" | "media" | "text" | "url" | "obs_scene";

export interface DeckButtonDef {
  id: string;
  label: string;
  icon: string;
  voice: string;
  action: { kind: DeckActionKind; value: string };
}

export interface DeckPageDef {
  name: string;
  buttons: DeckButtonDef[];
}

export interface ApiInfo {
  enabled: boolean;
  port: number | null;
  token: string;
}

export interface RemoteSettings {
  commands: SavedCommand[];
  deck: DeckPageDef[];
  apps: { id: string; name: string }[];
  obs_url: string;
  obs_password_set: boolean;
  api: ApiInfo;
}

// ------------------------------------------------------------------ Phase 4

export type ScreenSource = { kind: "monitor"; id: number } | { kind: "window"; id: number };

export interface SourceInfo {
  source: ScreenSource;
  name: string;
  detail: string;
  width: number;
  height: number;
  x: number;
  y: number;
  primary: boolean;
}

export interface ScreenSources {
  sources: SourceInfo[];
  selected: ScreenSource | null;
  wayland: boolean;
  viewers: string[];
}

export type Edge = "left" | "right" | "top" | "bottom";

export interface ScreenLayout {
  enabled: boolean;
  links: { device: string; edge: Edge }[];
}

export interface ScreenLayoutInfo {
  layout: ScreenLayout;
  supported: boolean;
  active: boolean;
}

// ------------------------------------------------------------------ Phase 5

export type AccessoryControl = { id: string; label: string; hint?: string } & (
  | { type: "toggle"; on: string; off: string }
  | { type: "choice"; options: { value: string; label: string; send: string }[] }
  | { type: "slider"; min: number; max: number; step: number; send: string }
  | { type: "equalizer"; bands: string[]; min: number; max: number; offset: number; send: string }
  | { type: "action"; send: string }
);

export interface Accessory {
  address: string;
  name: string;
  kind: "headphones" | "speaker" | "keyboard" | "mouse" | "gamepad" | "watch" | "other";
  paired: boolean;
  connected: boolean;
  rssi: number | null;
  battery: Record<string, { percent: number; charging: boolean }>;
  profile: {
    id: string;
    name: string;
    vendor: string;
    verified: boolean;
    notes: string;
    controls: AccessoryControl[];
    has_channel: boolean;
  };
  values: Record<string, string>;
  session: string;
  services: string[];
  manufacturer_ids: number[];
  drops: number;
  last_connected: number | null;
  remember: boolean;
}

export interface AccessoryList {
  status: string;
  list: Accessory[];
}

/** One decoded screen frame header + JPEG bytes. */
export interface ScreenFrame {
  seq: number;
  w: number;
  h: number;
  latencyMs: number;
  fps: number;
  jpeg: Uint8Array;
}

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

  // Phase 3 — control
  sendInput(id: string, events: InputEvent[]): Promise<void>;
  systemAction(id: string, action: SystemAction): Promise<void>;
  launchApp(id: string, app: string): Promise<void>;
  runRemoteCommand(id: string, command: string): Promise<void>;
  deckPress(id: string, button: string): Promise<void>;
  requestCatalog(id: string): Promise<void>;
  remoteSettings(): Promise<RemoteSettings>;
  saveCommands(commands: SavedCommand[]): Promise<void>;
  saveDeck(pages: DeckPageDef[]): Promise<void>;
  testCommand(command: string): Promise<[boolean, string]>;
  tryDeckAction(action: DeckButtonDef["action"]): Promise<void>;
  setObs(url: string, password: string | null): Promise<void>;
  setLocalApi(enabled: boolean): Promise<ApiInfo>;
  resetApiToken(): Promise<ApiInfo>;
  /** Starts a device's screen; frames arrive on `onFrame` until `screenUnwatch`. */
  screenWatch(id: string, maxWidth: number, fps: number, onFrame: (f: ScreenFrame) => void): Promise<void>;
  screenUnwatch(id: string): Promise<void>;
  /** Saves a screenshot/recording into the received-files folder; returns its path. */
  saveCapture(name: string, bytes: Uint8Array): Promise<string>;

  // Phase 4 — screens
  screenSources(): Promise<ScreenSources>;
  setScreenSource(source: ScreenSource | null): Promise<void>;
  stopScreenViewer(id: string): Promise<void>;
  screenLayout(): Promise<ScreenLayoutInfo>;
  setScreenLayout(layout: ScreenLayout): Promise<void>;

  // Phase 5 — Bluetooth accessories
  accessories(): Promise<AccessoryList>;
  onAccessories(cb: (list: AccessoryList) => void): () => void;
  setAccessoryControl(address: string, control: string, value: string): Promise<void>;
  setAccessoryRemember(address: string, on: boolean): Promise<void>;
  connectAccessory(address: string, on: boolean): Promise<void>;
  accessoryBatteryHistory(address: string, since: number): Promise<BatterySample[]>;
  helpSupportAccessory(address: string): Promise<string>;

  // Phase 6 — communication and automation
  requestSmsThreads(id: string): Promise<void>;
  requestSmsMessages(id: string, thread: string): Promise<void>;
  sendSms(id: string, to: string[], text: string): Promise<void>;
  callAction(id: string, call: string, action: string): Promise<void>;
  handoff(id: string, item: HandoffItem): Promise<void>;
  /** Continue what's playing / the copied link on a device; returns what was sent. */
  handoffCurrent(id: string): Promise<string>;
  mediaStatus(): Promise<MediaStatus>;
  startPhoneCamera(id: string): Promise<void>;
  stopPhoneCamera(id: string): Promise<void>;
  automation(): Promise<AutomationInfo>;
  saveRules(rules: Rule[]): Promise<void>;
  runRuleNow(id: string): Promise<void>;

  // Phase 7 — reach and extensibility
  pickFolder(title: string): Promise<string | null>;
  reachInfo(): Promise<ReachInfo>;
  anywhereInfo(): Promise<AnywhereInfo>;
  setAnywhere(enabled: boolean, relay: string): Promise<AnywhereInfo>;
  setReach(enabled: boolean, extra: string[]): Promise<ReachInfo>;
  peerReach(id: string): Promise<string[]>;
  sharedFolders(): Promise<SharedFolder[]>;
  setSharedFolders(folders: SharedFolder[]): Promise<void>;
  /** The answer arrives as an `fs_listing` event. */
  fsList(id: string, path: string): Promise<void>;
  /** The file arrives as an ordinary transfer (or an `fs_error` event). */
  fsGet(id: string, path: string): Promise<void>;
  plugins(): Promise<PluginInfo[]>;
  pluginInspect(path: string): Promise<PluginInfo>;
  pluginInstall(path: string): Promise<PluginInfo>;
  pluginSetEnabled(id: string, enabled: boolean): Promise<void>;
  pluginRemove(id: string): Promise<void>;
  pluginRun(id: string, command: string): Promise<string>;
}
