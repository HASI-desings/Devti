import { useEffect, useState } from "react";
import type { MediaStatus, PeerSnapshot, RadioName } from "../../bridge";
import { Icon, type IconName } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./PhoneRadios.module.css";

const err = (e: unknown) => String(e instanceof Error ? e.message : e);

const RADIOS: { key: RadioName; label: string; icon: IconName; warnOff?: string; warnOn?: string }[] = [
  {
    key: "wifi",
    label: "Wi‑Fi",
    icon: "signal",
    warnOff: "Without Wi‑Fi the phone stays connected over mobile data, if it has any.",
  },
  {
    key: "data",
    label: "Mobile data",
    icon: "globe",
    warnOff: "Away from Wi‑Fi, the phone can't be reached until data is back on.",
  },
  { key: "bluetooth", label: "Bluetooth", icon: "bluetooth" },
  { key: "hotspot", label: "Hotspot", icon: "network" },
  {
    key: "airplane",
    label: "Airplane mode",
    icon: "rocket",
    warnOn:
      "Airplane mode cuts every connection: the phone can't be reached until it's turned off on the phone.",
  },
];

/** Device page → a phone's radios, switched from this computer. */
export function PhoneRadios({ peer }: { peer: PeerSnapshot }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const radios = peer.status.radios;
  const [busy, setBusy] = useState<RadioName | null>(null);
  const [confirm, setConfirm] = useState<{ radio: RadioName; on: boolean; text: string } | null>(null);

  useEffect(() => {
    return bridge.onEvent((e) => {
      if (e.type === "radio_result" && e.from === peer.info.id) {
        setBusy(null);
        toast(e.message, e.ok ? "ok" : "info");
      }
    });
  }, [bridge, peer.info.id, toast]);

  if (!radios) return null;
  const send = async (radio: RadioName, on: boolean) => {
    setConfirm(null);
    setBusy(radio);
    try {
      await bridge.setRadio(peer.info.id, radio, on);
      window.setTimeout(() => setBusy((b) => (b === radio ? null : b)), 10_000);
    } catch (e) {
      setBusy(null);
      toast(err(e), "danger");
    }
  };
  const ask = (radio: RadioName, on: boolean) => {
    const r = RADIOS.find((x) => x.key === radio)!;
    const warn = on ? r.warnOn : r.warnOff;
    if (warn) setConfirm({ radio, on, text: warn });
    else void send(radio, on);
  };

  return (
    <div className={p.item}>
      <div className={s.wrap}>
        <div className={p.itemText}>
          <span className={p.itemTitle}>Its radios</span>
          <span className={p.itemHint}>
            {radios.via === "shizuku"
              ? "Switched straight from here (Shizuku is on for Devti on the phone)."
              : radios.via === "device_owner"
                ? "Wi‑Fi switches from here. For the rest, the phone shows its switch to whoever holds it, or install Shizuku on the phone."
                : "Android lets apps switch none of these by themselves: the phone opens the switch for whoever holds it. With Shizuku on the phone, they switch from here."}
          </span>
        </div>
        <div className={s.grid}>
          {RADIOS.map((r) => {
            const value = radios[r.key];
            const direct = radios.direct.includes(r.key);
            return (
              <div key={r.key} className={s.radio} data-on={value || undefined}>
                <Icon name={r.icon} size={18} />
                <span className={s.label}>
                  {r.label}
                  {!direct && <span className={s.via}>opens on the phone</span>}
                </span>
                <Switch
                  isSelected={!!value}
                  isDisabled={!peer.online || busy === r.key || value == null}
                  onChange={(on) => ask(r.key, on)}
                >
                  <span className="visually-hidden">{r.label}</span>
                </Switch>
              </div>
            );
          })}
        </div>
        {confirm && (
          <div className={s.confirm} role="alertdialog" aria-label="Are you sure?">
            <p>{confirm.text}</p>
            <div className={p.row}>
              <Button size="sm" variant="ghost" onPress={() => setConfirm(null)}>
                Cancel
              </Button>
              <Button size="sm" variant="danger" onPress={() => void send(confirm.radio, confirm.on)}>
                Turn {confirm.on ? "on" : "off"}
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

const clock = (sec: number) => {
  const m = Math.floor(sec / 60);
  const ss = Math.floor(sec % 60);
  return `${m}:${String(ss).padStart(2, "0")}`;
};

/** Device page → record the phone's microphone or its sound into Music/Devti. */
export function PhoneRecorder({ peer }: { peer: PeerSnapshot }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const id = peer.info.id;
  const [media, setMedia] = useState<MediaStatus | null>(null);
  const [, setTick] = useState(0);

  useEffect(() => {
    void bridge.mediaStatus().then(setMedia, () => {});
    const off = bridge.onMedia(setMedia);
    // The timer ticks while recording.
    const t = window.setInterval(() => {
      setTick((n) => n + 1);
      void bridge.mediaStatus().then(setMedia, () => {});
    }, 1000);
    return () => {
      off();
      window.clearInterval(t);
    };
  }, [bridge]);

  const recs = media?.recordings.filter((r) => r.device === id) ?? [];
  const rec = (kind: "mic" | "relay") => recs.find((r) => r.kind === kind);
  const toggle = async (kind: "mic" | "relay") => {
    try {
      if (rec(kind)) {
        const path = await bridge.recordStop(id, kind);
        toast(path ? `Recording saved: ${path}` : "Nothing was recorded.", path ? "ok" : "info");
      } else {
        await bridge.recordStart(id, kind);
      }
    } catch (e) {
      toast(err(e), "danger");
    }
  };

  return (
    <div className={p.item}>
      <div className={p.itemText}>
        <span className={p.itemTitle}>Record it</span>
        <span className={p.itemHint} data-testid="record-status">
          {recs.length === 0
            ? `Its microphone or what it's playing, saved as a WAV file in ${media?.recordings_folder ?? "Music/Devti"}. The phone asks first.`
            : recs
                .map((r) =>
                  r.live
                    ? `Recording ${r.kind === "mic" ? "the microphone" : "its sound"} · ${clock(r.seconds)}`
                    : `Waiting for ${peer.info.name} to allow it…`,
                )
                .join(" · ")}
        </span>
      </div>
      <div className={p.row}>
        {(["mic", "relay"] as const).map((kind) => {
          const r = rec(kind);
          return (
            <Button
              key={kind}
              size="sm"
              variant={r ? "danger" : "quiet"}
              icon={r ? "pause" : "record"}
              isDisabled={!peer.online && !r}
              onPress={() => void toggle(kind)}
            >
              {r ? "Stop" : kind === "mic" ? "Microphone" : "Its sound"}
            </Button>
          );
        })}
      </div>
    </div>
  );
}
