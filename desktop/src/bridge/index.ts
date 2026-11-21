import { createPreviewBridge } from "./preview";
import { createTauriBridge } from "./tauri";
import type { Bridge } from "./types";

export * from "./types";

/** Picks the real Tauri bridge inside the app, preview mode in a plain browser. */
export function createBridge(): Bridge {
  const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return inTauri ? createTauriBridge() : createPreviewBridge();
}
