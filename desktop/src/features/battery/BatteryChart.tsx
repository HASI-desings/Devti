import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { BatterySample } from "../../bridge";
import s from "./Battery.module.css";

const H = 168;
const PAD = { l: 40, r: 44, t: 12, b: 26 };
/** Samples further apart than this are a gap (device was off or out of reach). */
const GAP_MS = 75 * 60_000;

export interface ChartProps {
  samples: BatterySample[];
  since: number;
  until: number;
  /** CSS color for the line (a validated chart voice). */
  color: string;
  threshold: number;
  label: string;
}

/** Battery level over time for one device. Single series → the card title names it. */
export function BatteryChart({ samples, since, until, color, threshold, label }: ChartProps) {
  const wrap = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(560);
  const [hover, setHover] = useState<number | null>(null);

  useLayoutEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => e && setW(Math.max(260, Math.round(e.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const iw = w - PAD.l - PAD.r;
  const ih = H - PAD.t - PAD.b;
  const x = (t: number) => PAD.l + ((t - since) / Math.max(1, until - since)) * iw;
  const y = (p: number) => PAD.t + (1 - p / 100) * ih;

  const { path, bands } = (() => {
    let d = "";
    let prev: BatterySample | null = null;
    const bands: [number, number][] = [];
    let bandStart: number | null = null;
    for (const smp of samples) {
      const gap = !prev || smp.ts - prev.ts > GAP_MS;
      d += `${gap ? "M" : "L"}${x(smp.ts).toFixed(1)} ${y(smp.percent).toFixed(1)} `;
      if (smp.charging && bandStart === null) bandStart = smp.ts;
      if (!smp.charging && bandStart !== null) {
        bands.push([bandStart, smp.ts]);
        bandStart = null;
      }
      prev = smp;
    }
    if (bandStart !== null && prev) bands.push([bandStart, prev.ts]);
    return { path: d.trim(), bands };
  })();

  const ticks = useMemo(() => {
    const span = until - since;
    const day = 24 * 3600_000;
    const out: { t: number; label: string }[] = [];
    if (span <= day + 1) {
      const start = new Date(since);
      start.setMinutes(0, 0, 0);
      start.setHours(Math.ceil(start.getHours() / 6) * 6);
      for (let t = start.getTime(); t <= until; t += 6 * 3600_000) {
        out.push({
          t,
          label: new Date(t).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }),
        });
      }
    } else {
      const start = new Date(since);
      start.setHours(0, 0, 0, 0);
      for (let t = start.getTime() + day; t <= until; t += day) {
        out.push({ t, label: new Date(t).toLocaleDateString(undefined, { weekday: "short" }) });
      }
    }
    return out;
  }, [since, until]);

  const last = samples.at(-1);
  const hovered = hover !== null ? samples[hover] : null;

  const onMove = (e: React.PointerEvent<SVGRectElement>) => {
    if (!samples.length) return;
    const r = (e.currentTarget as SVGRectElement).getBoundingClientRect();
    const t = since + ((e.clientX - r.left) / r.width) * (until - since);
    let best = 0;
    for (let i = 1; i < samples.length; i++) {
      if (Math.abs(samples[i]!.ts - t) < Math.abs(samples[best]!.ts - t)) best = i;
    }
    setHover(best);
  };

  return (
    <div ref={wrap} className={s.chartWrap}>
      <svg
        width={w}
        height={H}
        role="img"
        aria-label={`${label}: battery level over time`}
        className={s.chart}
      >
        {bands.map(([a, b]) => (
          <rect
            key={a}
            x={x(a)}
            y={PAD.t}
            width={Math.max(2, x(b) - x(a))}
            height={ih}
            className={s.band}
            style={{ fill: color }}
          />
        ))}
        {[0, 50, 100].map((v) => (
          <g key={v}>
            <line x1={PAD.l} x2={PAD.l + iw} y1={y(v)} y2={y(v)} className={s.grid} />
            <text x={PAD.l - 8} y={y(v) + 4} className={s.axis} textAnchor="end">
              {v}%
            </text>
          </g>
        ))}
        <line x1={PAD.l} x2={PAD.l + iw} y1={y(threshold)} y2={y(threshold)} className={s.threshold} />
        <text x={PAD.l + iw} y={y(threshold) - 5} className={s.axis} textAnchor="end">
          Alert at {threshold}%
        </text>
        {ticks.map((tk) => (
          <text key={tk.t} x={x(tk.t)} y={H - 6} className={s.axis} textAnchor="middle">
            {tk.label}
          </text>
        ))}
        {path && <path d={path} className={s.line} style={{ stroke: color }} />}
        {last && (
          <g>
            <circle
              cx={x(last.ts)}
              cy={y(last.percent)}
              r={5}
              className={s.lastDot}
              style={{ fill: color }}
            />
            <text x={x(last.ts) + 9} y={y(last.percent) + 4} className={s.lastLabel}>
              {last.percent}%
            </text>
          </g>
        )}
        {hovered && (
          <g pointerEvents="none">
            <line x1={x(hovered.ts)} x2={x(hovered.ts)} y1={PAD.t} y2={PAD.t + ih} className={s.cross} />
            <circle
              cx={x(hovered.ts)}
              cy={y(hovered.percent)}
              r={5}
              className={s.lastDot}
              style={{ fill: color }}
            />
          </g>
        )}
        <rect
          x={PAD.l}
          y={PAD.t}
          width={iw}
          height={ih}
          fill="transparent"
          onPointerMove={onMove}
          onPointerLeave={() => setHover(null)}
        />
      </svg>
      {hovered && (
        <div className={s.tip} style={{ left: Math.min(Math.max(x(hovered.ts), 70), w - 70) }} role="status">
          <strong className="num">{hovered.percent}%</strong>
          <span>
            {new Date(hovered.ts).toLocaleString(undefined, {
              weekday: "short",
              hour: "2-digit",
              minute: "2-digit",
            })}
          </span>
          {hovered.charging && <span>Charging</span>}
        </div>
      )}
    </div>
  );
}
