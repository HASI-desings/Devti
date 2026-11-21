import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { BluetoothEvent, Bridge, NodeEvent } from "./types";

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
  };
}
