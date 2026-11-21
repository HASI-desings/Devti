import type { DeviceKind, Platform } from "../bridge";

export function relativeTime(ts: number | null | undefined, now = Date.now()): string {
  if (!ts) return "never";
  const s = Math.round((now - ts) / 1000);
  if (s < 10) return "just now";
  if (s < 60) return `${s} seconds ago`;
  const m = Math.round(s / 60);
  if (m < 60) return m === 1 ? "a minute ago" : `${m} minutes ago`;
  const h = Math.round(m / 60);
  if (h < 24) return h === 1 ? "an hour ago" : `${h} hours ago`;
  const d = Math.round(h / 24);
  if (d < 7) return d === 1 ? "yesterday" : `${d} days ago`;
  return new Date(ts).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

export function clockTime(ts: number): string {
  return new Date(ts).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function kindLabel(kind: DeviceKind): string {
  return (
    (
      {
        desktop: "Desktop",
        laptop: "Laptop",
        phone: "Phone",
        tablet: "Tablet",
        accessory: "Accessory",
      } as Record<string, string>
    )[kind] ?? "Device"
  );
}

export function platformLabel(p: Platform): string {
  return (
    (
      {
        windows: "Windows",
        linux: "Linux",
        macos: "macOS",
        android: "Android",
        ios: "iOS",
        ipados: "iPadOS",
      } as Record<string, string>
    )[p] ?? "Other"
  );
}

/** Plain-language quality for a round-trip time. */
export function latencyLabel(ms: number): string {
  if (ms <= 15) return "Excellent";
  if (ms <= 40) return "Good";
  if (ms <= 90) return "Fair";
  return "Slow";
}

const ACTIONS: Record<string, string> = {
  paired: "Paired",
  connected: "Connected",
  disconnected: "Disconnected",
  revoked: "Removed",
  revoked_by_peer: "Removed this computer",
  permissions_changed: "Permissions changed",
  ecosystem_paused: "Ecosystem paused",
  ecosystem_resumed: "Ecosystem resumed",
};

export function actionLabel(action: string): string {
  return ACTIONS[action] ?? action.replace(/_/g, " ");
}

/** "482913" or "482 913" → "482 913". */
export function formatCode(code: string): string {
  const d = code.replace(/\D/g, "");
  return d.length === 6 ? `${d.slice(0, 3)} ${d.slice(3)}` : code;
}
