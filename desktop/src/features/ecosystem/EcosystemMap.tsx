import {
  animate,
  type MotionValue,
  motion,
  useMotionValue,
  useReducedMotion,
  useSpring,
  useTransform,
} from "motion/react";
import {
  type CSSProperties,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import type { LocalSnapshot, PeerSnapshot } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
import { Prism } from "../../design/Prism";
import { hash, type Voice, voiceVar } from "../../design/voice";
import { relativeTime } from "../../lib/format";
import { Button } from "../../ui";
import s from "./EcosystemMap.module.css";

type Point = { x: number; y: number };

interface Props {
  local: LocalSnapshot;
  peers: PeerSnapshot[];
  voices: Record<string, Voice>;
  justLinked: Record<string, number>;
  selected: string | null;
  onSelect: (id: string | null) => void;
  onPair: () => void;
  onResume: () => void;
}

/** Where each device sits: online devices on the inner ring, offline ones drift outward. */
export function layoutRing(peers: PeerSnapshot[], w: number, h: number): Record<string, Point> {
  const cx = w / 2;
  const cy = h / 2;
  const rx = Math.min(w * 0.31, 320);
  const ry = Math.min(h * 0.3, 230);
  const out: Record<string, Point> = {};
  const place = (list: PeerSnapshot[], sx: number, sy: number, phase: number) => {
    list.forEach((p, i) => {
      const a = -Math.PI / 2 + phase + (i / Math.max(list.length, 1)) * Math.PI * 2;
      out[p.info.id] = {
        x: Math.round(cx + Math.cos(a) * rx * sx),
        y: Math.round(cy + Math.sin(a) * ry * sy),
      };
    });
  };
  const byPairing = [...peers].sort((a, b) => a.paired_at - b.paired_at);
  const online = byPairing.filter((p) => p.online);
  const offline = byPairing.filter((p) => !p.online);
  place(online, 1, 1, online.length === 2 ? Math.PI / 2 : online.length % 2 ? 0 : Math.PI / online.length);
  place(offline, 1.34, 1.3, Math.PI / Math.max(offline.length * 2, 2) + 0.35);
  // Keep everything inside the canvas with room for labels.
  for (const id of Object.keys(out)) {
    const p = out[id]!;
    p.x = Math.min(Math.max(p.x, 70), w - 70);
    p.y = Math.min(Math.max(p.y, 70), h - 90);
  }
  return out;
}

export function EcosystemMap({
  local,
  peers,
  voices,
  justLinked,
  selected,
  onSelect,
  onPair,
  onResume,
}: Props) {
  const reduce = useReducedMotion();
  const wrap = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 900, h: 640 });
  const [layer, setLayer] = useState<SVGGElement | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);

  useLayoutEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      if (!e) return;
      const { width, height } = e.contentRect;
      if (width > 0 && height > 0) setSize({ w: Math.round(width), h: Math.round(height) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Pointer parallax, smoothed by a spring. Disabled with reduced motion.
  const px = useMotionValue(0);
  const py = useMotionValue(0);
  const spx = useSpring(px, { stiffness: 60, damping: 18, mass: 0.6 });
  const spy = useSpring(py, { stiffness: 60, damping: 18, mass: 0.6 });

  const nodes = useRef(
    new Map<string, { x: MotionValue<number>; y: MotionValue<number>; voice: Voice; online: boolean }>(),
  );
  const frame = useRef(0);
  const draw = useCallback(() => {
    frame.current = 0;
    drawField(canvas.current, size, nodes.current, spx.get(), spy.get());
  }, [size, spx, spy]);
  const requestDraw = useCallback(() => {
    if (!frame.current) frame.current = requestAnimationFrame(draw);
  }, [draw]);

  useEffect(() => {
    requestDraw();
    const a = spx.on("change", requestDraw);
    const b = spy.on("change", requestDraw);
    return () => {
      a();
      b();
      cancelAnimationFrame(frame.current);
      frame.current = 0;
    };
  }, [requestDraw, spx, spy]);

  const onPointerMove = (e: React.PointerEvent) => {
    if (reduce) return;
    const r = wrap.current?.getBoundingClientRect();
    if (!r) return;
    px.set(((e.clientX - r.left) / r.width - 0.5) * 2);
    py.set(((e.clientY - r.top) / r.height - 0.5) * 2);
  };
  const onPointerLeave = () => {
    px.set(0);
    py.set(0);
  };

  const positions = useMemo(() => layoutRing(peers, size.w, size.h), [peers, size.w, size.h]);
  const center = { x: size.w / 2, y: size.h / 2 };
  const onlineVoices = peers.filter((p) => p.online).map((p) => voices[p.info.id] ?? "iris");
  const onlineCount = onlineVoices.length;

  // Hub parallax (it sits deepest, so it moves least).
  const hubX = useTransform(spx, (v) => v * -4);
  const hubY = useTransform(spy, (v) => v * -4);

  return (
    <div
      ref={wrap}
      className={s.map}
      data-paused={local.paused || undefined}
      onPointerMove={onPointerMove}
      onPointerLeave={onPointerLeave}
      data-tour="map"
    >
      <canvas ref={canvas} className={s.field} aria-hidden />
      <svg className={s.threads} width={size.w} height={size.h} aria-hidden>
        <g ref={setLayer} />
      </svg>

      {layer &&
        peers.map((p, i) => (
          <Orbit
            key={p.info.id}
            peer={p}
            index={i}
            target={positions[p.info.id] ?? center}
            center={center}
            voice={voices[p.info.id] ?? "iris"}
            layer={layer}
            px={spx}
            py={spy}
            linked={!!justLinked[p.info.id]}
            selected={selected === p.info.id}
            onSelect={onSelect}
            register={(id, v) => {
              if (v) nodes.current.set(id, v);
              else nodes.current.delete(id);
              requestDraw();
            }}
            requestDraw={requestDraw}
            reduce={!!reduce}
          />
        ))}

      <motion.div className={s.hub} style={{ left: center.x, top: center.y, x: hubX, y: hubY }}>
        <button
          type="button"
          className={s.hubButton}
          onClick={() => onSelect(null)}
          aria-label={`${local.info.name}, this computer. ${onlineCount} devices connected.`}
        >
          <Prism size={136} voices={onlineVoices.slice(-3)} />
        </button>
        <div className={s.hubLabel}>
          <span className={s.hubName}>{local.info.name}</span>
          <span className={s.hubSub}>This computer</span>
        </div>
      </motion.div>

      {peers.length === 0 && !local.paused && (
        <div className={s.empty} style={{ top: center.y + 128 }}>
          <svg
            className={s.ghostRing}
            width={size.w}
            height={size.h}
            aria-hidden
            style={{ top: -(center.y + 128) }}
          >
            <ellipse
              cx={center.x}
              cy={center.y}
              rx={Math.min(size.w * 0.31, 320)}
              ry={Math.min(size.h * 0.3, 230)}
              className={s.ghostEllipse}
            />
          </svg>
          <p className={s.emptyText}>Your ecosystem starts with one more device.</p>
          <Button variant="primary" icon="link" onPress={onPair}>
            Pair your first device
          </Button>
        </div>
      )}

      {local.paused && (
        <div className={s.pausedVeil}>
          <Icon name="pause" size={28} />
          <p className={s.pausedTitle}>Ecosystem paused</p>
          <p className={s.pausedText}>No device can connect to this computer until you resume.</p>
          <Button variant="primary" icon="play" onPress={onResume}>
            Resume ecosystem
          </Button>
        </div>
      )}

      <p className={s.caption} aria-live="polite">
        {peers.length === 0
          ? "No devices paired yet"
          : `${onlineCount} of ${peers.length} ${peers.length === 1 ? "device" : "devices"} connected`}
      </p>
    </div>
  );
}

function Orbit({
  peer,
  index,
  target,
  center,
  voice,
  layer,
  px,
  py,
  linked,
  selected,
  onSelect,
  register,
  requestDraw,
  reduce,
}: {
  peer: PeerSnapshot;
  index: number;
  target: Point;
  center: Point;
  voice: Voice;
  layer: SVGGElement;
  px: MotionValue<number>;
  py: MotionValue<number>;
  linked: boolean;
  selected: boolean;
  onSelect: (id: string | null) => void;
  register: (
    id: string,
    v: { x: MotionValue<number>; y: MotionValue<number>; voice: Voice; online: boolean } | null,
  ) => void;
  requestDraw: () => void;
  reduce: boolean;
}) {
  const id = peer.info.id;
  // Each device floats at its own depth, so parallax separates the layers.
  const depth = 0.55 + (hash(id) % 45) / 100;
  const bx = useMotionValue(center.x);
  const by = useMotionValue(center.y);
  const x = useTransform(() => bx.get() + px.get() * 14 * depth);
  const y = useTransform(() => by.get() + py.get() * 10 * depth);
  const hubX = useTransform(px, (v) => center.x + v * -4);
  const hubY = useTransform(py, (v) => center.y + v * -4);

  useEffect(() => {
    const opts = reduce
      ? { duration: 0 }
      : { type: "spring" as const, stiffness: 70, damping: 15, mass: 0.9 };
    const a = animate(bx, target.x, opts);
    const b = animate(by, target.y, opts);
    return () => {
      a.stop();
      b.stop();
    };
  }, [target.x, target.y, bx, by, reduce]);

  useEffect(() => {
    register(id, { x, y, voice, online: peer.online });
    const ux = x.on("change", requestDraw);
    const uy = y.on("change", requestDraw);
    return () => {
      ux();
      uy();
      register(id, null);
    };
  }, [id, x, y, voice, peer.online, register, requestDraw]);

  // Light travels from the device into the prism: path starts at the device.
  const bend = index % 2 ? 0.12 : -0.12;
  const d = useTransform(() => {
    const [x1, y1, x2, y2] = [x.get(), y.get(), hubX.get(), hubY.get()];
    const mx = (x1 + x2) / 2 - (y2 - y1) * bend;
    const my = (y1 + y2) / 2 + (x2 - x1) * bend;
    return `M${x1.toFixed(1)} ${y1.toFixed(1)} Q${mx.toFixed(1)} ${my.toFixed(1)} ${x2.toFixed(1)} ${y2.toFixed(1)}`;
  });

  const color = voiceVar(voice);
  const battery = peer.status.battery;
  const style = { "--voice": color } as CSSProperties;
  const label = `${peer.info.name}, ${peer.online ? "connected" : "offline"}${
    battery ? `, battery ${battery.percent} percent` : ""
  }`;

  return (
    <>
      {createPortal(
        <g style={style} className={peer.online ? s.threadOnline : s.threadOffline}>
          {peer.online && <motion.path d={d} className={s.threadGlow} />}
          <motion.path
            d={d}
            className={s.thread}
            initial={linked && !reduce ? { pathLength: 0 } : false}
            animate={{ pathLength: 1 }}
            transition={{ duration: 0.9, ease: [0.22, 1, 0.36, 1] }}
          />
          {peer.online && !reduce && (
            <motion.path
              d={d}
              pathLength={1}
              className={s.pulse}
              style={{ animationDelay: `${linked ? 0.6 : (hash(id) % 40) / 10}s` }}
            />
          )}
        </g>,
        layer,
      )}
      <motion.div
        className={s.node}
        style={{ ...style, x, y }}
        data-online={peer.online || undefined}
        data-selected={selected || undefined}
        initial={reduce ? false : { scale: 0.6, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: "spring", stiffness: 260, damping: 22 }}
      >
        <button
          type="button"
          className={s.lens}
          onClick={() => onSelect(selected ? null : id)}
          aria-pressed={selected}
          aria-label={label}
        >
          <svg className={s.ring} viewBox="0 0 76 76" aria-hidden>
            <circle cx="38" cy="38" r="35" className={s.ringTrack} />
            {battery && (
              <circle
                cx="38"
                cy="38"
                r="35"
                className={s.ringFill}
                pathLength={100}
                strokeDasharray={`${battery.percent} 100`}
                transform="rotate(-90 38 38)"
              />
            )}
          </svg>
          <Icon name={deviceIcon(peer.info.kind)} size={30} dotColor={peer.online ? color : undefined} />
          {linked && !reduce && <span className={s.flash} />}
        </button>
        <span className={s.name}>{peer.info.name}</span>
        <span className={s.sub}>
          {peer.online
            ? battery
              ? `${battery.percent}%${battery.charging ? " charging" : ""}`
              : "Connected"
            : `Seen ${relativeTime(peer.last_seen)}`}
        </span>
      </motion.div>
    </>
  );
}

/** Dot field: a quiet grid that bends and tints around each device, like light near mass. */
function drawField(
  c: HTMLCanvasElement | null,
  size: { w: number; h: number },
  nodes: Map<string, { x: MotionValue<number>; y: MotionValue<number>; voice: Voice; online: boolean }>,
  pxv: number,
  pyv: number,
) {
  if (!c) return;
  const ctx = c.getContext("2d");
  if (!ctx) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  if (c.width !== size.w * dpr || c.height !== size.h * dpr) {
    c.width = size.w * dpr;
    c.height = size.h * dpr;
    c.style.width = `${size.w}px`;
    c.style.height = `${size.h}px`;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, size.w, size.h);
  const css = getComputedStyle(c);
  const dot = css.getPropertyValue("--dot").trim() || "rgba(200,190,255,.16)";
  const voiceColors = new Map<Voice, string>();
  const masses = [...nodes.values()].map((n) => {
    if (!voiceColors.has(n.voice))
      voiceColors.set(n.voice, css.getPropertyValue(`--voice-${n.voice}`).trim());
    return { x: n.x.get(), y: n.y.get(), color: voiceColors.get(n.voice)!, online: n.online };
  });
  masses.push({ x: size.w / 2, y: size.h / 2, color: "", online: true });
  const gap = 26;
  const ox = (pxv * 5) % gap;
  const oy = (pyv * 4) % gap;
  for (let gx = -gap; gx < size.w + gap; gx += gap) {
    for (let gy = -gap; gy < size.h + gap; gy += gap) {
      let x = gx + ox;
      let y = gy + oy;
      let tint = "";
      let strength = 0;
      for (const m of masses) {
        const dx = x - m.x;
        const dy = y - m.y;
        const dist = Math.hypot(dx, dy);
        const reach = m.color ? 110 : 150;
        if (dist < reach && dist > 0.1) {
          const f = (1 - dist / reach) ** 2;
          x += (dx / dist) * f * 12;
          y += (dy / dist) * f * 12;
          if (m.color && m.online && f > strength) {
            strength = f;
            tint = m.color;
          }
        }
      }
      ctx.globalAlpha = tint ? 0.25 + strength * 0.75 : 1;
      ctx.fillStyle = tint || dot;
      ctx.beginPath();
      ctx.arc(x, y, tint ? 1 + strength * 0.9 : 1, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  ctx.globalAlpha = 1;
}
