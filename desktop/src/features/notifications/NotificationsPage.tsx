import { AnimatePresence, motion } from "motion/react";
import { type CSSProperties, useMemo, useState } from "react";
import type { MirroredNotification } from "../../bridge";
import { Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { relativeTime } from "../../lib/format";
import { useStore } from "../../state/store";
import { Button, Field, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Notifications.module.css";

export function NotificationsPage() {
  const bridge = useBridge();
  const notifications = useStore((st) => st.notifications);
  const dnd = useStore((st) => st.dnd);
  const peers = useStore((st) => st.peers);
  const groups = useMemo(() => {
    const m = new Map<string, MirroredNotification[]>();
    for (const n of [...notifications].sort((a, b) => b.notif.ts - a.notif.ts)) {
      m.set(n.from, [...(m.get(n.from) ?? []), n]);
    }
    return [...m.entries()];
  }, [notifications]);
  const anyAndroid = Object.values(peers).some((x) => x.info.platform === "android");

  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Notifications</h1>
          <p className={p.intro}>
            Your phone's notifications, here on your computer. Reply, act or dismiss — it happens on the phone
            too.
          </p>
        </header>

        <div className={s.dnd} data-on={dnd || undefined}>
          <Icon name="moon" size={22} />
          <div className={p.itemText}>
            <span className={p.itemTitle}>Do Not Disturb, everywhere</span>
            <span className={p.itemHint}>
              {dnd
                ? "Notifications still arrive here, silently. Every connected device stays quiet too."
                : "Silence notifications on every connected device at once."}
            </span>
          </div>
          <Switch isSelected={dnd} onChange={(v) => void bridge.setDnd(v)}>
            <span className="visually-hidden">Do Not Disturb</span>
          </Switch>
        </div>

        {groups.length === 0 ? (
          <div className={s.empty}>
            <Icon name="bell" size={28} />
            <p className={p.itemTitle}>No notifications right now</p>
            <p className={p.sectionIntro}>
              {anyAndroid
                ? "On your Android phone, open Devti → Settings and allow notification access to mirror notifications here."
                : "Pair an Android phone and allow notification access in its Devti app. iPhones can't share their notifications with other apps — Apple doesn't allow it."}
            </p>
          </div>
        ) : (
          groups.map(([from, list]) => <Group key={from} from={from} list={list} />)
        )}
      </div>
    </div>
  );
}

function Group({ from, list }: { from: string; list: MirroredNotification[] }) {
  const voice = useStore((st) => st.voices[from]);
  const name = list[0]?.from_name ?? "";
  return (
    <section
      className={p.section}
      style={{ "--voice": voiceVar(voice ?? "iris") } as CSSProperties}
      aria-label={`From ${name}`}
    >
      <h2 className={s.groupTitle}>
        <span className={s.dot} aria-hidden />
        {name}
        <span className={s.count}>{list.length}</span>
      </h2>
      <ul className={s.list}>
        <AnimatePresence initial={false}>
          {list.map((n) => (
            <Card key={n.notif.key} n={n} />
          ))}
        </AnimatePresence>
      </ul>
    </section>
  );
}

function Card({ n }: { n: MirroredNotification }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [replying, setReplying] = useState<string | null>(null);
  const [text, setText] = useState("");
  const icon = useMemo(() => {
    if (!n.notif.icon_png?.length) return null;
    const bytes = Uint8Array.from(n.notif.icon_png);
    let bin = "";
    for (const b of bytes) bin += String.fromCharCode(b);
    return `data:image/png;base64,${btoa(bin)}`;
  }, [n.notif.icon_png]);

  const run = async (fn: () => Promise<void>, done?: string) => {
    try {
      await fn();
      if (done) toast(done, "ok");
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  };

  return (
    <motion.li
      layout
      className={s.card}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, x: 24 }}
      transition={{ duration: 0.22 }}
    >
      <div className={s.icon} aria-hidden>
        {icon ? <img src={icon} alt="" /> : <span>{n.notif.app_name.slice(0, 1)}</span>}
      </div>
      <div className={s.body}>
        <div className={s.top}>
          <span className={s.app}>{n.notif.app_name}</span>
          <span className={s.time}>{relativeTime(n.notif.ts)}</span>
        </div>
        {n.notif.title && <p className={s.title}>{n.notif.title}</p>}
        {n.notif.text && (
          <p className={s.text} data-selectable>
            {n.notif.text}
          </p>
        )}
        {replying ? (
          <div className={s.reply}>
            <Field
              label="Reply"
              value={text}
              onChange={setText}
              autoFocus
              placeholder="Type a reply"
              onSubmit={() =>
                void run(async () => {
                  await bridge.notificationAction(n.from, n.notif.key, replying, text);
                  setReplying(null);
                  setText("");
                }, "Reply sent.")
              }
            />
            <div className={s.actions}>
              <Button
                variant="primary"
                size="sm"
                isDisabled={!text.trim()}
                onPress={() =>
                  void run(async () => {
                    await bridge.notificationAction(n.from, n.notif.key, replying, text);
                    setReplying(null);
                    setText("");
                  }, "Reply sent.")
                }
              >
                Send reply
              </Button>
              <Button variant="ghost" size="sm" onPress={() => setReplying(null)}>
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <div className={s.actions}>
            {n.notif.actions.map((a) => (
              <Button
                key={a.id}
                size="sm"
                onPress={() =>
                  a.reply
                    ? setReplying(a.id)
                    : void run(
                        () => bridge.notificationAction(n.from, n.notif.key, a.id, null),
                        `${a.label} done.`,
                      )
                }
              >
                {a.label}
              </Button>
            ))}
            {n.notif.dismissable && (
              <Button
                size="sm"
                variant="ghost"
                onPress={() => void run(() => bridge.dismissNotification(n.from, n.notif.key))}
              >
                Dismiss
              </Button>
            )}
          </div>
        )}
      </div>
    </motion.li>
  );
}
