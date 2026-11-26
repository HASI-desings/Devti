import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AccessoryList, BluetoothEvent, Bridge, MediaStatus, NodeEvent, ScreenFrame } from "./types";

/** 12-byte header (seq u32, w u16, h u16, latency u16, fps u8, 0) + JPEG; see commands.rs. */
export function decodeFrame(buf: ArrayBuffer): ScreenFrame | null {
  if (buf.byteLength < 12) return null;
  const v = new DataView(buf);
  return {
    seq: v.getUint32(0, true),
    w: v.getUint16(4, true),
    h: v.getUint16(6, true),
    latencyMs: v.getUint16(8, true),
    fps: v.getUint8(10),
    jpeg: new Uint8Array(buf, 12),
  };
}

/** The real bridge: every call goes to the Rust core running inside the Hub. */
export function createTauriBridge(): Bridge {
  return {
    mode: "tauri",
    snapshot: () => invoke("snapshot"),
    systemInfo: () => invoke("system_info"),
    onEvent(cb) {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      listen<NodeEvent>("devti://event", (e) => cb(e.payload)).then((u) => {
        if (cancelled) u();
        else unlisten = u;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    openPairingWindow: () => invoke("open_pairing_window"),
    closePairingWindow: () => invoke("close_pairing_window"),
    confirmPairing: (pairingId, accept) => invoke("confirm_pairing", { pairingId, accept }),
    pairWithCode: (deviceId, code) => invoke("pair_with_code", { deviceId, code }),
    pairWithCodeAt: (address, code) => invoke("pair_with_code_at", { address, code }),
    revoke: (id) => invoke("revoke", { id }),
    setAccess: (id, role, permissions) => invoke("set_access", { id, role, permissions }),
    setPaused: (paused) => invoke("set_paused", { paused }),
    rename: (name) => invoke("rename", { name }),
    activity: (limit, beforeId, deviceId) => invoke("activity", { limit, beforeId, deviceId }),
    setSetting: (key, value) => invoke("set_setting", { key, value }),
    setAutostart: (enabled) => invoke("set_autostart", { enabled }),
    async bluetoothScan(cb) {
      const unlisten = await listen<BluetoothEvent>("devti://bluetooth", (e) => cb(e.payload));
      await invoke("bluetooth_scan_start");
      return () => {
        unlisten();
        void invoke("bluetooth_scan_stop");
      };
    },
    openExternal: (url) => openUrl(url),
    exportDiagnostics: () => invoke("export_diagnostics"),
    onUiAction(cb) {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      listen<string>("devti://ui", async () => {
        const action = await invoke<string | null>("take_ui_action");
        if (action) cb(action);
      }).then((u) => {
        if (cancelled) u();
        else unlisten = u;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    takeUiAction: () => invoke("take_ui_action"),
    takePendingSend: () => invoke("take_pending_send"),
    clipboardHistory: () => invoke("clipboard_history"),
    clearClipboardHistory: () => invoke("clear_clipboard_history"),
    copyToClipboard: (text) => invoke("copy_to_clipboard", { text }),
    shareClipboard: (text) => invoke("share_clipboard", { text }),
    ring: (id, on) => invoke("ring", { id, on }),
    notificationAction: (id, key, action, reply) => invoke("notification_action", { id, key, action, reply }),
    dismissNotification: (id, key) => invoke("dismiss_notification", { id, key }),
    setDnd: (on) => invoke("set_dnd", { on }),
    pickFiles: () => invoke("pick_files"),
    sendFiles: (id, paths) => invoke("send_files", { id, paths }),
    transfers: () => invoke("transfers"),
    cancelTransfer: (id) => invoke("cancel_transfer", { id }),
    retryTransfer: (id) => invoke("retry_transfer", { id }),
    clearFinishedTransfers: () => invoke("clear_finished_transfers"),
    openPath: (path) => invoke("open_path", { path }),
    revealPath: (path) => invoke("reveal_path", { path }),
    chooseDownloadsDir: () => invoke("choose_downloads_dir"),
    batteryHistory: (id, since) => invoke("battery_history", { id, since }),
    setLowBatteryThreshold: (percent) => invoke("set_low_battery_threshold", { percent }),
    onFileDrop(cb) {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      getCurrentWebview()
        .onDragDropEvent((e) => {
          if (e.payload.type === "drop") {
            const ratio = window.devicePixelRatio || 1;
            cb(e.payload.paths, e.payload.position.x / ratio, e.payload.position.y / ratio);
          }
        })
        .then((u) => {
          if (cancelled) u();
          else unlisten = u;
        });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    sendInput: (id, events) => invoke("send_input", { id, events }),
    systemAction: (id, action) => invoke("system_action", { id, action }),
    launchApp: (id, app) => invoke("launch_app", { id, app }),
    runRemoteCommand: (id, command) => invoke("run_remote_command", { id, command }),
    deckPress: (id, button) => invoke("deck_press", { id, button }),
    requestCatalog: (id) => invoke("request_catalog", { id }),
    remoteSettings: () => invoke("remote_settings"),
    saveCommands: (commands) => invoke("save_commands", { commands }),
    saveDeck: (pages) => invoke("save_deck", { pages }),
    testCommand: (command) => invoke("test_command", { command }),
    tryDeckAction: (action) => invoke("try_deck_action", { action }),
    setObs: (url, password) => invoke("set_obs", { url, password }),
    setLocalApi: (enabled) => invoke("set_local_api", { enabled }),
    resetApiToken: () => invoke("reset_api_token"),
    async screenWatch(id, maxWidth, fps, onFrame) {
      const frames = new Channel<ArrayBuffer>();
      frames.onmessage = (buf) => {
        const f = decodeFrame(buf);
        if (f) onFrame(f);
      };
      await invoke("screen_watch", { id, maxWidth, fps, frames });
    },
    screenUnwatch: (id) => invoke("screen_unwatch", { id }),
    saveCapture: (name, bytes) => invoke("save_capture", bytes, { headers: { "x-devti-name": name } }),
    screenSources: () => invoke("screen_sources"),
    setScreenSource: (source) => invoke("set_screen_source", { source }),
    stopScreenViewer: (id) => invoke("stop_screen_viewer", { id }),
    screenLayout: () => invoke("screen_layout"),
    setScreenLayout: (layout) => invoke("set_screen_layout", { layout }),
    accessories: () => invoke("accessories"),
    onAccessories(cb) {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      listen<AccessoryList>("devti://accessories", (e) => cb(e.payload)).then((u) => {
        if (cancelled) u();
        else unlisten = u;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    setAccessoryControl: (address, control, value) =>
      invoke("set_accessory_control", { address, control, value }),
    setAccessoryRemember: (address, on) => invoke("set_accessory_remember", { address, on }),
    connectAccessory: (address, on) => invoke("connect_accessory", { address, on }),
    accessoryBatteryHistory: (address, since) => invoke("accessory_battery_history", { address, since }),
    helpSupportAccessory: (address) => invoke("help_support_accessory", { address }),
    requestSmsThreads: (id) => invoke("request_sms_threads", { id }),
    requestSmsMessages: (id, thread) => invoke("request_sms_messages", { id, thread }),
    sendSms: (id, to, text) => invoke("send_sms", { id, to, text }),
    callAction: (id, call, action) => invoke("call_action", { id, call, action }),
    handoff: (id, item) => invoke("handoff", { id, item }),
    handoffCurrent: (id) => invoke("handoff_current", { id }),
    mediaStatus: () => invoke("media_status"),
    startPhoneCamera: (id, facing) => invoke("use_phone_camera", { id, facing: facing ?? null }),
    stopPhoneCamera: (id) => invoke("stop_phone_camera", { id }),
    async cameraView(id, onFrame) {
      const frames = new Channel<ArrayBuffer>();
      frames.onmessage = (buf) => {
        const f = decodeFrame(buf);
        if (f) onFrame(f);
      };
      await invoke("camera_view", { id, frames });
    },
    setupWebcam: () => invoke("setup_webcam"),
    onMedia(cb) {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      listen<MediaStatus>("devti://media", (e) => cb(e.payload)).then((u) => {
        if (cancelled) u();
        else unlisten = u;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    onNotice(cb) {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      listen<string>("devti://notice", (e) => cb(e.payload)).then((u) => {
        if (cancelled) u();
        else unlisten = u;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    automation: () => invoke("automation"),
    saveRules: (rules) => invoke("save_rules", { rules }),
    runRuleNow: (id) => invoke("run_rule_now", { id }),
    pickFolder: (title) => invoke("pick_folder", { title }),
    reachInfo: () => invoke("reach_info"),
    anywhereInfo: () => invoke("anywhere_info"),
    locateDevice: (id) => invoke("locate_device", { id }),
    deviceLocations: () => invoke("device_locations"),
    ownLocation: () => invoke("own_location"),
    setAnywhere: (enabled, relay) => invoke("set_anywhere", { enabled, relay }),
    setReach: (enabled, extra) => invoke("set_reach", { enabled, extra }),
    peerReach: (id) => invoke("peer_reach", { id }),
    sharedFolders: () => invoke("shared_folders"),
    setSharedFolders: (folders) => invoke("set_shared_folders", { folders }),
    fsList: (id, path) => invoke("fs_list", { id, path }),
    fsGet: (id, path) => invoke("fs_get", { id, path }),
    plugins: () => invoke("plugins"),
    pluginInspect: (path) => invoke("plugin_inspect", { path }),
    pluginInstall: (path) => invoke("plugin_install", { path }),
    pluginSetEnabled: (id, enabled) => invoke("plugin_set_enabled", { id, enabled }),
    pluginRemove: (id) => invoke("plugin_remove", { id }),
    pluginRun: (id, command) => invoke("plugin_run", { id, command }),
  };
}
