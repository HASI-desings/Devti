import { useEffect, useRef, useState } from "react";
import type { CameraFacing, CameraState, MediaStatus, PeerSnapshot } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button, Sheet } from "../../ui";
import p from "../shell/page.module.css";
import s from "./PhoneCamera.module.css";

const err = (e: unknown) => String(e instanceof Error ? e.message : e);

/**
 * Device page → the phone's camera. It always shows live in Devti; on Linux it can also
 * be the "Devti Camera" every video app sees. States: off → waiting for the phone's OK
 * → live (front or back) → off.
 */
export function PhoneCamera({ peer }: { peer: PeerSnapshot }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const id = peer.info.id;
  const [media, setMedia] = useState<MediaStatus | null>(null);
  const [facing, setFacing] = useState<CameraFacing>("front");
  const [viewing, setViewing] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void bridge.mediaStatus().then(setMedia, () => {});
    return bridge.onMedia(setMedia);
  }, [bridge]);

  const cam: CameraState | undefined = media?.cameras.find((c) => c.device === id);
  useEffect(() => {
    if (cam?.facing) setFacing(cam.facing);
    if (!cam) setViewing(false);
  }, [cam]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      toast(err(e), "danger");
    } finally {
      setBusy(false);
    }
  };

  const start = (f: CameraFacing) =>
    run(async () => {
      setFacing(f);
      await bridge.startPhoneCamera(id, f);
    });

  const status = !cam
    ? "Shows the phone's camera here. The phone asks before it turns on."
    : !cam.live
      ? `Waiting for ${peer.info.name}… Allow the camera on the phone.`
      : `On: ${cam.facing === "back" ? "back" : "front"} camera${
          cam.webcam ? ", also as “Devti Camera” for other apps" : ""
        }.`;

  return (
    <div className={p.item}>
      <div className={p.itemText}>
        <span className={p.itemTitle}>
          <span className={s.dot} data-state={cam ? (cam.live ? "live" : "waiting") : "off"} aria-hidden />
          Its camera
        </span>
        <span className={p.itemHint} aria-live="polite" data-testid="camera-status">
          {status}
        </span>
        {media && (
          <span className={p.itemHint}>
            {media.webcam}{" "}
            {media.webcam_setup && (
              <Button
                size="sm"
                variant="ghost"
                isDisabled={busy}
                onPress={() =>
                  run(async () => {
                    setMedia(await bridge.setupWebcam());
                    toast("The virtual webcam is ready. Pick “Devti Camera” in your video app.", "ok");
                  })
                }
              >
                Set up virtual webcam
              </Button>
            )}
          </span>
        )}
      </div>
      <div className={p.row}>
        <fieldset className={s.facing} aria-label="Which camera">
          {(["front", "back"] as const).map((f) => (
            <button
              key={f}
              type="button"
              className={s.facingButton}
              aria-pressed={facing === f}
              disabled={busy}
              onClick={() => (cam ? void start(f) : setFacing(f))}
            >
              {f === "front" ? "Front" : "Back"}
            </button>
          ))}
        </fieldset>
        {!cam ? (
          <Button size="sm" icon="camera" isDisabled={busy} onPress={() => void start(facing)}>
            Use camera
          </Button>
        ) : (
          <>
            <Button size="sm" icon="eye" isDisabled={!cam.live} onPress={() => setViewing(true)}>
              Show
            </Button>
            <Button size="sm" variant="danger" onPress={() => void run(() => bridge.stopPhoneCamera(id))}>
              Stop
            </Button>
          </>
        )}
      </div>
      <Sheet
        isOpen={viewing && !!cam?.live}
        onClose={() => setViewing(false)}
        label={`${peer.info.name} camera`}
      >
        {cam?.live && (
          <CameraView
            id={id}
            name={peer.info.name}
            facing={cam.facing ?? "front"}
            onSwitch={() => void start(cam.facing === "back" ? "front" : "back")}
          />
        )}
      </Sheet>
    </div>
  );
}

/** The live picture, drawn the same way as Mirror: newest frame only, decoded off-thread. */
function CameraView({
  id,
  name,
  facing,
  onSwitch,
}: {
  id: string;
  name: string;
  facing: CameraFacing;
  onSwitch: () => void;
}) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [fps, setFps] = useState(0);
  const [flip, setFlip] = useState(facing === "front");
  const [turn, setTurn] = useState(0);

  useEffect(() => setFlip(facing === "front"), [facing]);

  useEffect(() => {
    let alive = true;
    let drawing = false;
    bridge
      .cameraView(id, async (f) => {
        setFps(f.fps);
        if (drawing || !alive) return;
        drawing = true;
        try {
          const bmp = await createImageBitmap(new Blob([f.jpeg as BlobPart], { type: "image/jpeg" }));
          const c = canvas.current;
          if (c && alive) {
            if (c.width !== bmp.width || c.height !== bmp.height) {
              c.width = bmp.width;
              c.height = bmp.height;
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
      .catch((e) => toast(err(e), "danger"));
    return () => {
      alive = false;
    };
  }, [bridge, id, toast]);

  return (
    <div className={s.view}>
      <div className={s.stage}>
        <canvas
          ref={canvas}
          className={s.canvas}
          style={{ transform: `rotate(${turn}deg) scaleX(${flip ? -1 : 1})` }}
          aria-label={`Live picture from ${name}'s ${facing} camera`}
          data-testid="camera-canvas"
        />
      </div>
      <div className={s.bar}>
        <span className={s.fps}>
          <Icon name="camera" size={16} /> {facing === "back" ? "Back" : "Front"} camera ·{" "}
          <span className="num">{fps}</span> fps
        </span>
        <div className={p.row}>
          <Button size="sm" variant="ghost" onPress={() => setFlip((v) => !v)}>
            Mirror
          </Button>
          <Button size="sm" variant="ghost" onPress={() => setTurn((t) => (t + 90) % 360)}>
            Rotate
          </Button>
          <Button size="sm" onPress={onSwitch}>
            Switch to {facing === "back" ? "front" : "back"}
          </Button>
        </div>
      </div>
    </div>
  );
}
