import { useEffect, useState } from "react";
import type { Accessory, SoundStatus } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { orderedPeers, useStore } from "../../state/store";
import { Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./SoundCard.module.css";

const err = (e: unknown) => String(e instanceof Error ? e.message : e);

interface Choice {
  key: string;
  mode: SoundStatus["mode"];
  device?: string;
  title: string;
  hint: string;
  disabled?: boolean;
}

/**
 * Accessories → one pair of headphones for every device, and music that pauses for
 * voice messages.
 */
export function SoundCard({ accessories }: { accessories: Accessory[] }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const phones = orderedPeers(useStore((st) => st.peers)).filter(
    (x) => x.info.platform === "android" && x.online,
  );
  const [status, setStatus] = useState<SoundStatus | null>(null);

  useEffect(() => {
    void bridge.soundStatus().then(setStatus, () => {});
    return bridge.onSound(setStatus);
  }, [bridge]);

  if (!status) return null;
  const headset = accessories.find((a) => a.connected && (a.kind === "headphones" || a.kind === "speaker"));
  const choices: Choice[] = [
    {
      key: "separate",
      mode: "separate",
      title: "Each device plays its own sound",
      hint: "As usual: the headphones play whichever device they're connected to.",
    },
    ...phones.flatMap((ph): Choice[] => [
      {
        key: `here:${ph.info.id}`,
        mode: "here",
        device: ph.info.id,
        title: `${ph.info.name}'s sound here, together with this computer's`,
        hint: headset
          ? `Both play in your ${headset.name}. ${ph.info.name} asks once before sharing its sound.`
          : `Both play on this computer's speakers or headphones. ${ph.info.name} asks once before sharing its sound.`,
      },
      {
        key: `on_phone:${ph.info.id}`,
        mode: "on_phone",
        device: ph.info.id,
        title: `This computer's sound on ${ph.info.name}`,
        hint: `For headphones connected to ${ph.info.name}: both devices play there together.`,
        disabled: !status.can_send,
      },
    ]),
  ];
  const current = status.mode === "separate" ? "separate" : `${status.mode}:${status.device}`;

  const choose = async (c: Choice) => {
    try {
      setStatus(await bridge.setSoundMode(c.mode, c.device));
    } catch (e) {
      toast(err(e), "danger");
    }
  };

  return (
    <section className={s.card} aria-labelledby="sound-title">
      <div className={s.head}>
        <span className={s.icon}>
          <Icon name="headphones" size={26} />
        </span>
        <div>
          <h2 id="sound-title" className={s.title}>
            One pair of headphones for everything
          </h2>
          <p className={p.itemHint}>
            A game here and a song on your phone, in the same headphones at the same time.
          </p>
        </div>
      </div>

      <fieldset className={s.choices}>
        <legend className="visually-hidden">Where everything plays</legend>
        {choices.map((c) => (
          <label
            key={c.key}
            className={s.choice}
            data-checked={current === c.key || undefined}
            data-disabled={c.disabled || undefined}
          >
            <input
              type="radio"
              name="sound-mode"
              className={s.radio}
              checked={current === c.key}
              disabled={c.disabled}
              onChange={() => void choose(c)}
            />
            <span className={s.dot} aria-hidden />
            <span className={s.choiceText}>
              <span className={s.choiceTitle}>{c.title}</span>
              <span className={s.choiceHint}>{c.hint}</span>
            </span>
          </label>
        ))}
        {phones.length === 0 && (
          <p className={p.itemHint}>
            Connect your Android phone to share its sound here, or this computer's on it.
          </p>
        )}
      </fieldset>
      {status.note && <p className={s.note}>{status.note}</p>}

      <div className={p.item}>
        <div className={p.itemText}>
          <span className={p.itemTitle}>Pause the music for voice messages</span>
          <span className={p.itemHint} data-testid="voice-status">
            {status.paused_for_voice
              ? "A voice message is playing: the music is paused and comes back when it ends."
              : "A WhatsApp, Instagram or Telegram voice message on any device pauses the music on the others, and the music comes back by itself when it ends."}
          </span>
        </div>
        <Switch
          isSelected={status.voice_pause}
          onChange={(on) => void bridge.setVoicePause(on).then(setStatus, (e) => toast(err(e), "danger"))}
        >
          <span className="visually-hidden">Pause the music for voice messages</span>
        </Switch>
      </div>
      <p className={s.fine}>
        Sound from the network arrives about a tenth of a second late, fine for music and videos, noticeable
        in fast games. A few apps don't let Android share their sound; they keep playing on the phone itself.
      </p>
    </section>
  );
}
