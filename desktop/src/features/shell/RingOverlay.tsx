import { motion } from "motion/react";
import { useEffect } from "react";
import { Prism } from "../../design/Prism";
import { useStore } from "../../state/store";
import { Button } from "../../ui";
import s from "./RingOverlay.module.css";

/**
 * Find my device, on this computer: a full-screen call with a loud, repeating chime
 * until someone presses "I found it" (or the other device stops ringing).
 */
export function RingOverlay() {
  const ringing = useStore((st) => st.ringing);
  const stop = useStore((st) => st.stopRinging);

  useEffect(() => {
    if (!ringing) return;
    const Ctx =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctx) return;
    const ctx = new Ctx();
    let stopped = false;
    // A bright three-note chime (C6–E6–G6), repeated every 1.4 s.
    const chime = () => {
      if (stopped) return;
      const now = ctx.currentTime;
      [1046.5, 1318.5, 1568].forEach((f, i) => {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = "triangle";
        osc.frequency.value = f;
        gain.gain.setValueAtTime(0.0001, now + i * 0.14);
        gain.gain.exponentialRampToValueAtTime(0.6, now + i * 0.14 + 0.02);
        gain.gain.exponentialRampToValueAtTime(0.0001, now + i * 0.14 + 0.5);
        osc.connect(gain).connect(ctx.destination);
        osc.start(now + i * 0.14);
        osc.stop(now + i * 0.14 + 0.55);
      });
    };
    void ctx.resume();
    chime();
    const timer = setInterval(chime, 1400);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") stop();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      stopped = true;
      clearInterval(timer);
      window.removeEventListener("keydown", onKey);
      void ctx.close();
    };
  }, [ringing, stop]);

  if (!ringing) return null;
  return (
    <div
      className={s.overlay}
      role="alertdialog"
      aria-modal="true"
      aria-label={`${ringing.name} is looking for this computer`}
    >
      <motion.div
        className={s.halo}
        animate={{ scale: [1, 1.25, 1], opacity: [0.5, 0.15, 0.5] }}
        transition={{ duration: 1.4, repeat: Number.POSITIVE_INFINITY }}
        aria-hidden
      />
      <Prism size={180} voices={["coral", "marigold", "rose"]} />
      <h1 className={s.title}>Here I am</h1>
      <p className={s.text}>{ringing.name} is looking for this computer.</p>
      <Button variant="primary" size="lg" onPress={stop} autoFocus>
        I found it
      </Button>
    </div>
  );
}
