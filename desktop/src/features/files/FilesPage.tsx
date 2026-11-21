import { useCallback, useEffect, useState } from "react";
import type { ClipboardEntry } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { etaLabel, formatBytes } from "../../lib/bytes";
import { clockTime, relativeTime } from "../../lib/format";
import { useStore } from "../../state/store";
import { Button, Segmented, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Files.module.css";

type Tab = "transfers" | "clipboard";

export function FilesPage() {
  const [tab, setTab] = useState<Tab>("transfers");
  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Files and clipboard</h1>
          <p className={p.intro}>
            Drop files anywhere in Devti to send them. Copy on one device and paste on another.
          </p>
        </header>
        <Segmented<Tab>
          label="Show"
          value={tab}
          onChange={setTab}
          options={[
            { value: "transfers", label: "Transfers", icon: "folder" },
            { value: "clipboard", label: "Clipboard", icon: "clipboard" },
          ]}
        />
        {tab === "transfers" ? <Transfers /> : <Clipboard />}
      </div>
    </div>
  );
}

function Transfers() {
  const bridge = useBridge();
  const transfers = useStore((st) => st.transfers);
  const setTransfers = useStore((st) => st.setTransfers);
  const setSendPaths = useStore((st) => st.setSendPaths);
  const downloadsDir = useStore((st) => st.downloadsDir);
  const toast = useStore((st) => st.toast);
  const list = Object.values(transfers).sort((a, b) => b.ts - a.ts);

  useEffect(() => {
    void bridge.transfers().then(setTransfers);
  }, [bridge, setTransfers]);

  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  };

  return (
    <>
      <button
        type="button"
        className={s.drop}
        onClick={async () => {
          const paths = await bridge.pickFiles();
          if (paths.length) setSendPaths(paths);
        }}
        data-tour="drop"
      >
        <Icon name="download" size={28} style={{ transform: "rotate(180deg)" }} />
        <span className={s.dropTitle}>Drop files here, or choose files to send</span>
        <span className={s.muted}>Any size. Transfers resume by themselves if a device drops off.</span>
      </button>

      <section className={p.section} aria-labelledby="transfers">
        <div className={s.head}>
          <h2 id="transfers" className={p.sectionTitle}>
            Transfers
          </h2>
          {list.some((t) => ["done", "failed", "cancelled"].includes(t.state)) && (
            <Button
              variant="ghost"
              size="sm"
              onPress={() =>
                void act(async () => {
                  await bridge.clearFinishedTransfers();
                  setTransfers(await bridge.transfers());
                })
              }
            >
              Clear finished
            </Button>
          )}
        </div>
        {list.length === 0 ? (
          <p className={p.sectionIntro}>
            Nothing sent or received yet. Received files are saved in {downloadsDir || "Downloads/Devti"}.
          </p>
        ) : (
          <ul className={s.transfers}>
            {list.map((t) => {
              const pct = t.size ? Math.min(100, Math.round((t.done / t.size) * 100)) : 100;
              const running = t.state === "active";
              const eta = running ? etaLabel(t.size - t.done, t.rate) : null;
              return (
                <li key={t.id} className={s.transfer} data-state={t.state}>
                  <Icon name={t.incoming ? "download" : "external"} size={20} />
                  <div className={s.tMain}>
                    <div className={s.tTop}>
                      <span className={s.tName}>{t.name}</span>
                      <span className={s.muted}>
                        {t.incoming ? "from" : "to"} {t.device_name}
                      </span>
                    </div>
                    <div
                      className={s.bar}
                      role="progressbar"
                      aria-label={`${t.name} ${pct}%`}
                      aria-valuenow={pct}
                      aria-valuemin={0}
                      aria-valuemax={100}
                    >
                      <span style={{ width: `${pct}%` }} />
                    </div>
                    <div className={s.tMeta}>
                      <span className="num">
                        {formatBytes(t.done)} of {formatBytes(t.size)}
                      </span>
                      <span>{stateLabel(t.state, eta, t.rate)}</span>
                    </div>
                  </div>
                  <div className={s.tActions}>
                    {(t.state === "active" || t.state === "queued" || t.state === "paused") && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onPress={() => void act(() => bridge.cancelTransfer(t.id))}
                      >
                        Cancel
                      </Button>
                    )}
                    {!t.incoming && (t.state === "failed" || t.state === "paused") && (
                      <Button size="sm" onPress={() => void act(() => bridge.retryTransfer(t.id))}>
                        Retry
                      </Button>
                    )}
                    {t.incoming && t.state === "done" && (
                      <>
                        <Button size="sm" onPress={() => void act(() => bridge.openPath(t.path))}>
                          Open
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          onPress={() => void act(() => bridge.revealPath(t.path))}
                        >
                          Show in folder
                        </Button>
                      </>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </>
  );
}

function stateLabel(state: string, eta: string | null, rate: number) {
  switch (state) {
    case "active":
      return [rate > 0 ? `${formatBytes(rate)}/s` : "Starting", eta].filter(Boolean).join(", ");
    case "queued":
      return "Waiting for the device to come online";
    case "paused":
      return "Interrupted, resuming when possible";
    case "done":
      return "Done";
    case "failed":
      return "Failed";
    default:
      return "Cancelled";
  }
}

function Clipboard() {
  const bridge = useBridge();
  const settings = useStore((st) => st.settings);
  const setSetting = useStore((st) => st.setSetting);
  const toast = useStore((st) => st.toast);
  const [items, setItems] = useState<ClipboardEntry[]>([]);
  const load = useCallback(() => void bridge.clipboardHistory().then(setItems), [bridge]);

  useEffect(() => {
    load();
    return bridge.onEvent((e) => {
      if (e.type === "clipboard_received") load();
    });
  }, [bridge, load]);

  const on = settings.clipboard_sync !== "false";
  return (
    <section className={p.section} aria-labelledby="clip">
      <div className={p.panel}>
        <div className={p.item}>
          <div className={p.itemText}>
            <span className={p.itemTitle}>Share the clipboard</span>
            <span className={p.itemHint}>
              Text you copy here appears on your devices, and theirs here. Each device's permission decides
              whether it takes part.
            </span>
          </div>
          <Switch
            isSelected={on}
            onChange={(v) => {
              setSetting("clipboard_sync", String(v));
              void bridge.setSetting("clipboard_sync", String(v));
            }}
          >
            <span className="visually-hidden">Share the clipboard</span>
          </Switch>
        </div>
      </div>
      <div className={s.head}>
        <h2 id="clip" className={p.sectionTitle}>
          Recent clipboard
        </h2>
        {items.length > 0 && (
          <Button
            variant="ghost"
            size="sm"
            onPress={async () => {
              await bridge.clearClipboardHistory();
              load();
            }}
          >
            Clear history
          </Button>
        )}
      </div>
      {items.length === 0 ? (
        <p className={p.sectionIntro}>
          Nothing copied yet. The last 50 items are kept on this computer only.
        </p>
      ) : (
        <ul className={s.clips}>
          {items.map((c) => (
            <li key={c.id} className={s.clip}>
              <p className={s.clipText} data-selectable>
                {c.text.length > 400 ? `${c.text.slice(0, 400)}…` : c.text}
              </p>
              <div className={s.clipMeta}>
                <span className={s.muted} title={clockTime(c.ts)}>
                  {c.device_name ? `From ${c.device_name}` : "Copied here"}, {relativeTime(c.ts)}
                </span>
                <div className={s.tActions}>
                  <Button
                    size="sm"
                    variant="ghost"
                    icon="copy"
                    onPress={async () => {
                      await bridge.copyToClipboard(c.text);
                      toast("Copied.", "ok");
                    }}
                  >
                    Copy
                  </Button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
