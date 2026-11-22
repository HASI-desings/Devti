import { AnimatePresence, motion } from "motion/react";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button } from "../../ui";
import s from "./CallCard.module.css";

/**
 * A phone's incoming or active call, floating over the app: answer or decline from the
 * computer. The call's audio stays on the phone (or a headset paired with it) — Android
 * doesn't let apps carry call audio; see the help page for using this computer as the
 * phone's Bluetooth headset instead.
 */
export function CallCard() {
  const bridge = useBridge();
  const calls = useStore((st) => st.calls);
  const toast = useStore((st) => st.toast);
  const entry = Object.entries(calls).find(([, c]) => c.state === "ringing" || c.state === "active");
  const act = (phone: string, id: string, action: string) =>
    bridge
      .callAction(phone, id, action)
      .catch((e) => toast(String(e instanceof Error ? e.message : e), "danger"));
  return (
    <AnimatePresence>
      {entry && (
        <motion.div
          key={entry[1].id}
          className={s.card}
          role="alertdialog"
          aria-label={`Call from ${entry[1].name || entry[1].number}`}
          initial={{ opacity: 0, y: -24, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: -16 }}
          data-state={entry[1].state}
        >
          <span className={s.icon} aria-hidden="true">
            <Icon name="phoneCall" size={24} />
          </span>
          <div className={s.text}>
            <span className={s.who}>{entry[1].name || entry[1].number}</span>
            <span className={s.sub}>
              {entry[1].state === "ringing" ? "Incoming call" : "On a call"} · {entry[1].from_name}
              {entry[1].name ? ` · ${entry[1].number}` : ""}
            </span>
          </div>
          {entry[1].state === "ringing" ? (
            <div className={s.actions}>
              <Button variant="danger" onPress={() => act(entry[0], entry[1].id, "decline")}>
                Decline
              </Button>
              <Button variant="primary" onPress={() => act(entry[0], entry[1].id, "answer")}>
                Answer
              </Button>
            </div>
          ) : (
            <div className={s.actions}>
              <Button variant="ghost" onPress={() => act(entry[0], entry[1].id, "mute")}>
                Mute
              </Button>
              <Button variant="danger" onPress={() => act(entry[0], entry[1].id, "end")}>
                End
              </Button>
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
