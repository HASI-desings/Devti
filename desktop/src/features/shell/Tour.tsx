import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { useStore } from "../../state/store";
import { Button } from "../../ui";
import s from "./Tour.module.css";

const STOPS: { target: string; title: string; text: string }[] = [
  {
    target: "map",
    title: "Your ecosystem",
    text: "Each device is a point of light with its own color. A line means it's connected; light travels along it into this computer, the prism in the middle.",
  },
  {
    target: "panel",
    title: "Devices at a glance",
    text: "Battery, connection quality and access for every device. Select one on the map to see its details here.",
  },
  {
    target: "pair",
    title: "Add a device",
    text: "Show a QR code for phones and tablets, or a 6-digit code for other computers.",
  },
  {
    target: "nav-security",
    title: "Stay in control",
    text: "Choose what each device may do, read the activity log, or pause the whole ecosystem in one click.",
  },
  {
    target: "nav-help",
    title: "Help is always here",
    text: "Guides, what works on each system, and this tour again whenever you want it.",
  },
];

export function Tour({ onDone }: { onDone: () => void }) {
  const [i, setI] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const stop = STOPS[i]!;
  const setTour = useStore((st) => st.setTour);

  const finish = useCallback(() => {
    setTour(false);
    onDone();
  }, [onDone, setTour]);

  useLayoutEffect(() => {
    const measure = () => {
      const el = document.querySelector(`[data-tour="${stop.target}"]`);
      setRect(el ? el.getBoundingClientRect() : null);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [stop.target]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") finish();
      if (e.key === "ArrowRight") setI((x) => Math.min(x + 1, STOPS.length - 1));
      if (e.key === "ArrowLeft") setI((x) => Math.max(x - 1, 0));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [finish]);

  const pad = 8;
  const hole = rect
    ? {
        left: rect.left - pad,
        top: rect.top - pad,
        width: rect.width + pad * 2,
        height: rect.height + pad * 2,
      }
    : { left: window.innerWidth / 2, top: window.innerHeight / 2, width: 0, height: 0 };
  // Put the card beside the highlighted area, wherever there is room.
  const cardW = 340;
  const right = hole.left + hole.width + 16;
  const fitsRight = right + cardW < window.innerWidth - 16;
  const cardLeft = fitsRight ? right : Math.max(16, Math.min(hole.left, window.innerWidth - cardW - 16));
  const cardTop = fitsRight
    ? Math.min(Math.max(16, hole.top), window.innerHeight - 240)
    : Math.min(hole.top + hole.height + 16, window.innerHeight - 240);

  return (
    <div className={s.root} role="dialog" aria-modal="true" aria-label="Tour of Devti">
      <motion.div
        className={s.hole}
        animate={hole}
        transition={{ type: "spring", stiffness: 160, damping: 24 }}
        aria-hidden
      />
      <AnimatePresence mode="wait">
        <motion.div
          key={i}
          className={s.card}
          style={{ left: cardLeft, top: cardTop, width: cardW }}
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
        >
          <p className={s.count}>
            <span className="num">
              {i + 1} of {STOPS.length}
            </span>
          </p>
          <h2 className={s.title}>{stop.title}</h2>
          <p className={s.text}>{stop.text}</p>
          <div className={s.actions}>
            <Button variant="ghost" size="sm" onPress={finish}>
              End tour
            </Button>
            <div className={s.nav}>
              {i > 0 && (
                <Button size="sm" onPress={() => setI(i - 1)}>
                  Back
                </Button>
              )}
              <Button
                variant="primary"
                size="sm"
                autoFocus
                onPress={() => (i === STOPS.length - 1 ? finish() : setI(i + 1))}
              >
                {i === STOPS.length - 1 ? "Finish" : "Next"}
              </Button>
            </div>
          </div>
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
