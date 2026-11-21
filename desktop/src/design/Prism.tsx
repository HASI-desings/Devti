import { motion, useReducedMotion } from "motion/react";
import { useId } from "react";
import { type Voice, voiceVar } from "./voice";

/**
 * The Hub glyph: a faceted prism. Each facet takes the voice of a connected device,
 * so the Hub literally shows who it is in touch with. With nobody online it is plain
 * glass; its core is white light (every voice at once).
 */
export function Prism({
  size = 120,
  voices = [],
  glow = true,
}: {
  size?: number;
  voices?: Voice[];
  glow?: boolean;
}) {
  const uid = useId().replace(/:/g, "");
  const reduce = useReducedMotion();
  const facet = (i: number) => (voices.length ? voiceVar(voices[i % voices.length]!) : "var(--text-faint)");
  const A = [60, 10];
  const B = [10, 98];
  const C = [110, 98];
  const O = [62, 66];
  const pts = (...p: number[][]) => p.map((q) => q.join(",")).join(" ");
  const strength = voices.length ? 0.55 : 0.18;

  return (
    <svg width={size} height={size} viewBox="0 0 120 112" role="img" aria-label="This computer — the Hub">
      <defs>
        {[0, 1, 2].map((i) => (
          <linearGradient
            key={i}
            id={`${uid}-f${i}`}
            x1={i === 1 ? 1 : 0}
            y1="0"
            x2={i === 2 ? 1 : 0.5}
            y2="1"
          >
            <stop offset="0%" stopColor={facet(i)} stopOpacity={strength} />
            <stop offset="100%" stopColor={facet(i)} stopOpacity={strength * 0.25} />
          </linearGradient>
        ))}
        <radialGradient id={`${uid}-core`}>
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.95" />
          <stop offset="45%" stopColor="#ffffff" stopOpacity="0.3" />
          <stop offset="100%" stopColor="#ffffff" stopOpacity="0" />
        </radialGradient>
      </defs>
      <polygon points={pts(A, B, O)} fill={`url(#${uid}-f0)`} />
      <polygon points={pts(B, C, O)} fill={`url(#${uid}-f1)`} />
      <polygon points={pts(C, A, O)} fill={`url(#${uid}-f2)`} />
      <g stroke="var(--hub)" strokeWidth="1.6" strokeLinejoin="round" fill="none" opacity="0.9">
        <polygon points={pts(A, B, C)} />
        <polyline points={pts(A, O, B)} opacity="0.45" />
        <line x1={O[0]} y1={O[1]} x2={C[0]} y2={C[1]} opacity="0.45" />
      </g>
      {glow && (
        <motion.circle
          cx={O[0]}
          cy={O[1]}
          fill={`url(#${uid}-core)`}
          initial={{ r: 16 }}
          animate={reduce ? { r: 18 } : { r: [16, 21, 16] }}
          transition={
            reduce ? { duration: 0 } : { duration: 5.5, repeat: Number.POSITIVE_INFINITY, ease: "easeInOut" }
          }
        />
      )}
      <circle cx={O[0]} cy={O[1]} r="3.2" fill="var(--hub)" />
    </svg>
  );
}

/** Small brand mark: the prism with three voices, used in the rail and About. */
export function Logo({ size = 28 }: { size?: number }) {
  return <Prism size={size} voices={["iris", "aqua", "coral"]} glow={false} />;
}
