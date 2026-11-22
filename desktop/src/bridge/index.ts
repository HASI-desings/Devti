import { createTauriBridge } from "./tauri";
import type { Bridge } from "./types";

export * from "./types";

/**
 * Picks the real Tauri bridge inside the app, preview mode in a plain browser. The preview
 * simulation is loaded only in the browser, so the shipped app never carries it in memory.
 */
export async function createBridge(): Promise<Bridge> {
  const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  if (inTauri) return createTauriBridge();
  const { createPreviewBridge } = await import("./preview");
  return createPreviewBridge();
}
