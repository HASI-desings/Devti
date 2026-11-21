// Every device gets a "voice": one hue of the spectrum, stable for its lifetime.
// The Hub itself is white light — the sum of every voice.

export const VOICES = ["iris", "aqua", "coral", "mint", "marigold", "rose"] as const;
export type Voice = (typeof VOICES)[number];

/** FNV-1a 32-bit — tiny, stable, good spread for short IDs. */
export function hash(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/**
 * Assigns voices so devices paired earlier keep theirs and nobody shares a voice
 * until all six are taken. `ordered` must be sorted by pairing time.
 */
export function assignVoices(ordered: { id: string }[]): Record<string, Voice> {
  const out: Record<string, Voice> = {};
  const used = new Set<Voice>();
  for (const { id } of ordered) {
    const start = hash(id) % VOICES.length;
    let pick: Voice = VOICES[start]!;
    if (used.size < VOICES.length) {
      for (let k = 0; k < VOICES.length; k++) {
        const v = VOICES[(start + k) % VOICES.length]!;
        if (!used.has(v)) {
          pick = v;
          break;
        }
      }
    }
    used.add(pick);
    out[id] = pick;
  }
  return out;
}

/** CSS color for a voice that adapts to the theme (light hue on night, deep on day). */
export function voiceVar(v: Voice): string {
  return `var(--voice-${v})`;
}
