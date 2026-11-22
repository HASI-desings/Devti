import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { clockTime, relativeTime } from "../../lib/format";
import { orderedPeers, useStore } from "../../state/store";
import { Button } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Messages.module.css";

const err = (e: unknown) => String(e instanceof Error ? e.message : e);

/** Hub → Messages: an Android phone's texts, read and answered from this computer. */
export function MessagesPage() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const allPeers = useStore((st) => st.peers);
  const phones = orderedPeers(allPeers).filter((x) => x.info.platform === "android");
  const [phoneId, setPhoneId] = useState<string | null>(null);
  const phone = phones.find((x) => x.info.id === phoneId) ?? phones[0];
  const threadsByPhone = useStore((st) => st.smsThreads);
  const messagesByKey = useStore((st) => st.smsMessages);
  const threads = phone ? (threadsByPhone[phone.info.id] ?? []) : [];
  const [threadId, setThreadId] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);
  const thread = threads.find((t) => t.id === threadId) ?? null;

  useEffect(() => {
    if (phone?.online) bridge.requestSmsThreads(phone.info.id).catch((e) => toast(err(e), "danger"));
  }, [bridge, phone?.info.id, phone?.online, toast]);
  useEffect(() => {
    if (phone?.online && threadId) bridge.requestSmsMessages(phone.info.id, threadId).catch(() => {});
  }, [bridge, phone?.info.id, phone?.online, threadId]);

  if (!phone) {
    return (
      <div className={p.page}>
        <div className={p.inner}>
          <header className={p.hero}>
            <h1 className={p.title}>Messages</h1>
            <p className={p.intro}>Pair an Android phone to read and send its texts here.</p>
          </header>
        </div>
      </div>
    );
  }
  const messages = thread ? (messagesByKey[`${phone.info.id}/${thread.id}`] ?? []) : [];

  return (
    <div className={s.page}>
      <aside className={s.side} aria-label="Conversations">
        <div className={s.sideHead}>
          <h1 className={s.title}>Messages</h1>
          {phones.length > 1 && (
            <select
              className={s.phonePick}
              aria-label="Phone"
              value={phone.info.id}
              onChange={(e) => setPhoneId(e.target.value)}
            >
              {phones.map((x) => (
                <option key={x.info.id} value={x.info.id}>
                  {x.info.name}
                </option>
              ))}
            </select>
          )}
          <Button size="sm" icon="plus" onPress={() => setComposing(true)} isDisabled={!phone.online}>
            New
          </Button>
        </div>
        {!phone.online && (
          <p className={s.note}>{phone.info.name} is offline. Showing what was loaded before.</p>
        )}
        {phone.online && threads.length === 0 && (
          <p className={s.note}>
            Loading… If nothing appears, allow SMS in the Devti app on {phone.info.name} (Home → Calls and
            messages).
          </p>
        )}
        <ul className={s.threads}>
          {threads.map((t) => (
            <li key={t.id}>
              <button
                type="button"
                className={s.thread}
                aria-current={t.id === thread?.id || undefined}
                onClick={() => {
                  setThreadId(t.id);
                  setComposing(false);
                }}
              >
                <span className={s.avatar} aria-hidden="true">
                  {(t.names[0] ?? t.addresses[0] ?? "?").slice(0, 1)}
                </span>
                <span className={s.threadText}>
                  <span className={s.threadName}>{t.names[0] || t.addresses.join(", ")}</span>
                  <span className={s.snippet}>{t.snippet}</span>
                </span>
                <span className={s.threadMeta}>
                  <span>{relativeTime(t.ts)}</span>
                  {t.unread > 0 && (
                    <span className={s.unread}>
                      {t.unread}
                      <span className="visually-hidden"> unread</span>
                    </span>
                  )}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <section
        className={s.main}
        aria-label={thread ? `Conversation with ${thread.names[0] || thread.addresses[0]}` : "Conversation"}
      >
        {composing ? (
          <Compose phoneId={phone.info.id} onSent={() => setComposing(false)} />
        ) : thread ? (
          <Conversation
            phoneId={phone.info.id}
            to={thread.addresses}
            title={thread.names[0] || thread.addresses.join(", ")}
            messages={messages}
            online={phone.online}
          />
        ) : (
          <div className={s.empty}>
            <Icon name="message" size={36} />
            <p>
              Pick a conversation, or start a new one. Texts are sent by {phone.info.name}, from its number.
            </p>
          </div>
        )}
      </section>
    </div>
  );
}

function Conversation({
  phoneId,
  to,
  title,
  messages,
  online,
}: {
  phoneId: string;
  to: string[];
  title: string;
  messages: import("../../bridge").SmsMessage[];
  online: boolean;
}) {
  const end = useRef<HTMLDivElement>(null);
  const sorted = useMemo(() => [...messages].sort((a, b) => a.ts - b.ts), [messages]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll when the list grows
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [sorted.length]);
  return (
    <div className={s.conversation}>
      <header className={s.convHead}>
        <h2>{title}</h2>
        <span className={s.note}>{to.join(", ")}</span>
      </header>
      <div className={s.bubbles}>
        {sorted.map((m) => (
          <div key={m.id} className={s.bubble} data-out={m.outgoing || undefined}>
            <span>{m.body}</span>
            <time className={s.time}>{clockTime(m.ts)}</time>
          </div>
        ))}
        <div ref={end} />
      </div>
      <Reply phoneId={phoneId} to={to} disabled={!online} />
    </div>
  );
}

function Reply({ phoneId, to, disabled }: { phoneId: string; to: string[]; disabled: boolean }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [text, setText] = useState("");
  const send = () => {
    const t = text.trim();
    if (!t) return;
    bridge.sendSms(phoneId, to, t).then(
      () => setText(""),
      (e) => toast(err(e), "danger"),
    );
  };
  return (
    <form
      className={s.reply}
      onSubmit={(e) => {
        e.preventDefault();
        send();
      }}
    >
      <textarea
        className={s.input}
        aria-label="Message"
        placeholder={disabled ? "The phone is offline" : "Text message"}
        value={text}
        disabled={disabled}
        rows={1}
        maxLength={2000}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            send();
          }
        }}
      />
      <Button type="submit" variant="primary" isDisabled={disabled || !text.trim()}>
        Send
      </Button>
    </form>
  );
}

function Compose({ phoneId, onSent }: { phoneId: string; onSent: () => void }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [to, setTo] = useState("");
  const [text, setText] = useState("");
  return (
    <form
      className={s.compose}
      onSubmit={(e) => {
        e.preventDefault();
        const numbers = to
          .split(",")
          .map((x) => x.trim())
          .filter(Boolean);
        if (!numbers.length || !text.trim()) return;
        bridge.sendSms(phoneId, numbers, text.trim()).then(
          () => {
            toast("Sent.", "ok");
            onSent();
          },
          (e2) => toast(err(e2), "danger"),
        );
      }}
    >
      <h2 className={p.sectionTitle}>New message</h2>
      <label className={s.label}>
        To
        <input
          className={s.field}
          value={to}
          onChange={(e) => setTo(e.target.value)}
          placeholder="+92 300 1234567, …"
          inputMode="tel"
        />
      </label>
      <label className={s.label}>
        Message
        <textarea
          className={s.field}
          rows={4}
          value={text}
          maxLength={2000}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <div className={p.row}>
        <Button type="submit" variant="primary" isDisabled={!to.trim() || !text.trim()}>
          Send
        </Button>
        <Button variant="ghost" onPress={onSent}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
