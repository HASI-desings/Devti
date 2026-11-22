import type { CSSProperties, KeyboardEvent, PointerEvent, WheelEvent } from "react";
import { useEffect, useRef, useState } from "react";
import type { InputEvent } from "../../bridge";
import { Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button } from "../../ui";
import s from "./Mirror.module.css";

const LONG_PRESS_MS = 550;
const TAP_SLOP = 0.012;

/**
 * Hub → Android: the phone's live screen with full control. Click = tap, hold = long
 * press, drag = swipe, wheel = scroll, right-click = back, and the computer's keyboard
 * types into the phone. Needs the Devti app on the phone with screen sharing accepted
 * and "Devti control" turned on in Android's Accessibility settings.
 */
export function MirrorPage() {
  const bridge = useBridge();
  const id = useStore((st) => st.deviceId);
  const peer = useStore((st) => (id ? st.peers[id] : undefined));
  const voice = useStore((st) => (id ? st.voices[id] : undefined));
  const stats = useStore((st) => (id ? st.screenStats[id] : undefined));
  const navigate = useStore((st) => st.navigate);
  const toast = useStore((st) => st.toast);
  const canvas = useRef<HTMLCanvasElement>(null);
  const lastJpeg = useRef<Uint8Array | null>(null);
  const [size, setSize] = useState<{ w: number; h: number } | null>(null);
  const [recording, setRecording] = useState<MediaRecorder | null>(null);
  const press = useRef<{ x: number; y: number; t: number; long: boolean; timer: number } | null>(null);
  const lastWheel = useRef(0);
  const online = !!peer?.online;

  useEffect(() => {
    if (!id || !online) return;
    let alive = true;
    let drawing = false;
    bridge
      .screenWatch(id, 1280, 30, async (f) => {
        lastJpeg.current = f.jpeg;
        if (drawing || !alive) return; // drop frames while the previous one decodes
        drawing = true;
        try {
          const bmp = await createImageBitmap(new Blob([f.jpeg as BlobPart], { type: "image/jpeg" }));
          const c = canvas.current;
          if (c && alive) {
            if (c.width !== bmp.width || c.height !== bmp.height) {
              c.width = bmp.width;
              c.height = bmp.height;
              setSize({ w: bmp.width, h: bmp.height });
            }
            c.getContext("2d")?.drawImage(bmp, 0, 0);
          }
          bmp.close();
        } catch {
          // a corrupt frame: skip it
        } finally {
          drawing = false;
        }
      })
      .catch((e) => toast(String(e instanceof Error ? e.message : e), "danger"));
    return () => {
      alive = false;
      void bridge.screenUnwatch(id).catch(() => {});
    };
  }, [bridge, id, online, toast]);

  if (!id || !peer) return null;
  const send = (events: InputEvent[]) => {
    void bridge
      .sendInput(id, events)
      .catch((e) => toast(String(e instanceof Error ? e.message : e), "danger"));
  };
  const at = (e: { clientX: number; clientY: number }) => {
    const r = canvas.current?.getBoundingClientRect();
    if (!r) return { x: 0, y: 0 };
    return {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    };
  };

  const onDown = (e: PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    e.currentTarget.focus();
    const { x, y } = at(e);
    const timer = window.setTimeout(() => {
      if (press.current) {
        press.current.long = true;
        send([{ k: "long_press", x, y }]);
      }
    }, LONG_PRESS_MS);
    press.current = { x, y, t: performance.now(), long: false, timer };
  };
  const onMove = (e: PointerEvent<HTMLCanvasElement>) => {
    const p = press.current;
    if (!p) return;
    const { x, y } = at(e);
    if (Math.hypot(x - p.x, y - p.y) > TAP_SLOP) window.clearTimeout(p.timer);
  };
  const onUp = (e: PointerEvent<HTMLCanvasElement>) => {
    const p = press.current;
    press.current = null;
    if (!p) return;
    window.clearTimeout(p.timer);
    if (p.long) return;
    const { x, y } = at(e);
    const ms = Math.round(Math.min(1500, Math.max(60, performance.now() - p.t)));
    if (Math.hypot(x - p.x, y - p.y) <= TAP_SLOP) send([{ k: "tap", x, y }]);
    else send([{ k: "swipe", x1: p.x, y1: p.y, x2: x, y2: y, ms }]);
  };
  const onWheel = (e: WheelEvent<HTMLCanvasElement>) => {
    const now = performance.now();
    if (now - lastWheel.current < 180) return;
    lastWheel.current = now;
    const { x, y } = at(e);
    const d = Math.sign(e.deltaY) * 0.22;
    send([
      {
        k: "swipe",
        x1: x,
        y1: Math.min(0.95, Math.max(0.05, y)),
        x2: x,
        y2: Math.min(0.95, Math.max(0.05, y - d)),
        ms: 160,
      },
    ]);
  };
  const onKey = (e: KeyboardEvent<HTMLCanvasElement>) => {
    const named: Record<string, string> = {
      Enter: "enter",
      Backspace: "backspace",
      Tab: "tab",
      Escape: "escape",
      ArrowLeft: "left",
      ArrowRight: "right",
      ArrowUp: "up",
      ArrowDown: "down",
      Delete: "delete",
    };
    if (e.ctrlKey && e.key.toLowerCase() === "v") return; // paste goes through the clipboard sync
    const key = named[e.key];
    if (key) {
      e.preventDefault();
      send([{ k: "key", key, mods: [] }]);
    } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      send([{ k: "text", text: e.key }]);
    }
  };

  const screenshot = async () => {
    const jpeg = lastJpeg.current;
    if (!jpeg) return;
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    try {
      const where = await bridge.saveCapture(`${peer.info.name} ${stamp}.jpg`, jpeg);
      toast(`Screenshot saved: ${where}`, "ok");
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  };
  const toggleRecord = () => {
    if (recording) {
      recording.stop();
      setRecording(null);
      return;
    }
    const c = canvas.current;
    if (!c || typeof MediaRecorder === "undefined" || !("captureStream" in c)) {
      toast("Recording isn't supported by this system's web view.", "danger");
      return;
    }
    const type = ["video/webm;codecs=vp9", "video/webm;codecs=vp8", "video/webm", "video/mp4"].find((t) =>
      MediaRecorder.isTypeSupported(t),
    );
    if (!type) {
      toast("Recording isn't supported by this system's web view.", "danger");
      return;
    }
    const rec = new MediaRecorder(c.captureStream(30), { mimeType: type, videoBitsPerSecond: 6_000_000 });
    const chunks: Blob[] = [];
    rec.ondataavailable = (ev) => ev.data.size && chunks.push(ev.data);
    rec.onstop = async () => {
      const bytes = new Uint8Array(await new Blob(chunks, { type }).arrayBuffer());
      const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
      const ext = type.startsWith("video/mp4") ? "mp4" : "webm";
      try {
        const where = await bridge.saveCapture(`${peer.info.name} ${stamp}.${ext}`, bytes);
        toast(`Recording saved: ${where}`, "ok");
      } catch (e) {
        toast(String(e instanceof Error ? e.message : e), "danger");
      }
    };
    rec.start(1000);
    setRecording(rec);
  };

  const color = voiceVar(voice ?? "iris");
  return (
    <div className={s.page} style={{ "--voice": color } as CSSProperties}>
      <header className={s.bar}>
        <Button variant="ghost" size="sm" icon="chevronLeft" onPress={() => navigate("device", id)}>
          {peer.info.name}
        </Button>
        <div className={s.stats} aria-live="off">
          {stats ? (
            <>
              <span>
                <b className="num">{stats.fps}</b> fps
              </span>
              <span>
                <b className="num">{stats.latency_ms}</b> ms
              </span>
              <span>
                <b className="num">{(stats.kbps / 1000).toFixed(1)}</b> Mbit/s
              </span>
            </>
          ) : (
            <span>Connecting…</span>
          )}
        </div>
        <div className={s.tools}>
          <Button
            size="sm"
            variant="ghost"
            icon="camera"
            onPress={() => void screenshot()}
            isDisabled={!size}
          >
            Screenshot
          </Button>
          <Button
            size="sm"
            variant={recording ? "danger" : "ghost"}
            icon="record"
            onPress={toggleRecord}
            isDisabled={!size}
          >
            {recording ? "Stop recording" : "Record"}
          </Button>
        </div>
      </header>

      <div className={s.stage}>
        {!online && <p className={s.wait}>{peer.info.name} is offline.</p>}
        {online && !size && (
          <div className={s.wait}>
            <Icon name="cast" size={36} />
            <p>
              Waiting for {peer.info.name}… On the phone, tap <b>Start now</b> to share its screen with Devti.
            </p>
            <p className={s.hint}>
              To control it from here, also turn on <b>Devti control</b> in the phone's Settings →
              Accessibility.
            </p>
          </div>
        )}
        <div
          className={s.device}
          hidden={!size}
          style={
            size
              ? ({
                  aspectRatio: `${size.w} / ${size.h}`,
                  height: `min(100cqh, calc(100cqw * ${size.h} / ${size.w}))`,
                } as CSSProperties)
              : undefined
          }
        >
          <canvas
            ref={canvas}
            className={s.screen}
            tabIndex={0}
            aria-label={`${peer.info.name}'s screen. Click to tap, drag to swipe, type to enter text.`}
            onPointerDown={onDown}
            onPointerMove={onMove}
            onPointerUp={onUp}
            onPointerCancel={() => {
              if (press.current) window.clearTimeout(press.current.timer);
              press.current = null;
            }}
            onWheel={onWheel}
            onKeyDown={onKey}
            onContextMenu={(e) => {
              e.preventDefault();
              send([{ k: "nav", action: "back" }]);
            }}
          />
        </div>
      </div>

      <nav className={s.nav} aria-label="Phone buttons">
        <Button variant="ghost" icon="back" onPress={() => send([{ k: "nav", action: "back" }])}>
          Back
        </Button>
        <Button variant="ghost" icon="home" onPress={() => send([{ k: "nav", action: "home" }])}>
          Home
        </Button>
        <Button variant="ghost" icon="recents" onPress={() => send([{ k: "nav", action: "recents" }])}>
          Recent apps
        </Button>
        <Button variant="ghost" icon="bell" onPress={() => send([{ k: "nav", action: "notifications" }])}>
          Notifications
        </Button>
        <Button variant="ghost" icon="lock" onPress={() => send([{ k: "nav", action: "lock_screen" }])}>
          Lock
        </Button>
      </nav>
    </div>
  );
}
