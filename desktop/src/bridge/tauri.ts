import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
  };
}
