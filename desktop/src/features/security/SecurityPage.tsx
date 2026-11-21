import { useCallback, useEffect, useState } from "react";
import type { ActivityEntry } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { actionLabel, clockTime } from "../../lib/format";
import { orderedPeers, useStore } from "../../state/store";
import { Button } from "../../ui";
import p from "../shell/page.module.css";
import s from "./SecurityPage.module.css";

export function SecurityPage({ onPair }: { onPair: () => void }) {
  const bridge = useBridge();
  const local = useStore((st) => st.local)!;
  const peers = orderedPeers(useStore((st) => st.peers));
  const navigate = useStore((st) => st.navigate);
  const toast = useStore((st) => st.toast);
  const [log, setLog] = useState<ActivityEntry[]>([]);
  const [filter, setFilter] = useState<string>("");

  const loadLog = useCallback(async () => {
    try {
      setLog(await bridge.activity(200, undefined, filter || undefined));
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  }, [bridge, filter, toast]);

  useEffect(() => {
    void loadLog();
    const off = bridge.onEvent((e) => {
      if (e.type === "peer_changed" || e.type === "peer_removed" || e.type === "paused") void loadLog();
    });
    return off;
  }, [bridge, loadLog]);

  const exportLog = () => {
    const rows = [["time", "device", "event", "detail"]].concat(
      log.map((e) => [new Date(e.ts).toISOString(), e.device_name ?? "", actionLabel(e.action), e.detail]),
    );
    const csv = rows.map((r) => r.map((c) => `"${c.replace(/"/g, '""')}"`).join(",")).join("\n");
    const blob = new Blob([csv], { type: "text/csv" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `devti-activity-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Security</h1>
          <p className={p.intro}>
            Every connection is end-to-end encrypted and every device proved who it is when you paired it.
            Here you decide who stays.
          </p>
        </header>

        <section className={s.kill} data-paused={local.paused || undefined} data-tour="kill">
          <div className={s.killText}>
            <h2 className={p.sectionTitle}>{local.paused ? "Ecosystem paused" : "Ecosystem active"}</h2>
            <p className={p.sectionIntro}>
              {local.paused
                ? "Every device is disconnected and new connections are refused until you resume."
                : "Pausing disconnects every device at once and refuses new connections. Use it if a device is lost or something feels wrong."}
            </p>
          </div>
          <Button
            variant={local.paused ? "primary" : "danger"}
            size="lg"
            icon={local.paused ? "play" : "pause"}
            onPress={() => void bridge.setPaused(!local.paused)}
          >
            {local.paused ? "Resume ecosystem" : "Pause ecosystem"}
          </Button>
        </section>

        <section className={p.section} aria-labelledby="devices">
          <div className={s.sectionHead}>
            <h2 id="devices" className={p.sectionTitle}>
              Paired devices
            </h2>
            <Button icon="plus" onPress={onPair} isDisabled={local.paused}>
              Pair a device
            </Button>
          </div>
          <div className={p.panel}>
            {peers.length === 0 && (
              <div className={p.item}>
                <span className={p.itemHint}>No devices are paired with this computer.</span>
              </div>
            )}
            {peers.map((peer) => (
              <button
                type="button"
                key={peer.info.id}
                className={`${p.item} ${s.peerRow}`}
                onClick={() => navigate("device", peer.info.id)}
              >
                <span className={p.itemText}>
                  <span className={p.itemTitle}>{peer.info.name}</span>
                  <span className={p.itemHint}>
                    {peer.role === "trusted" ? "Trusted" : "Restricted"}, paired {clockTime(peer.paired_at)}
                  </span>
                </span>
                <Icon name="chevronRight" size={18} />
              </button>
            ))}
          </div>
        </section>

        <section className={p.section} aria-labelledby="identity">
          <h2 id="identity" className={p.sectionTitle}>
            This computer's identity
          </h2>
          {!local.secret_os_protected && (
            <div className={p.note} role="note">
              <Icon name="warning" size={20} />
              <span>
                No system keychain was found, so this computer's private key is stored in a file only your
                user account can read. Install GNOME Keyring or KWallet and restart Devti to move it into the
                keychain.
              </span>
            </div>
          )}
          {local.identity_origin === "regenerated" && (
            <div className={p.note} role="note">
              <Icon name="warning" size={20} />
              <span>
                This computer's key was missing, so Devti created a new one. Devices paired before need to be
                paired again.
              </span>
            </div>
          )}
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Security fingerprint</span>
                <span className={`${p.itemHint} ${p.mono}`}>{local.fingerprint}</span>
              </div>
            </div>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Private key stored in</span>
                <span className={p.itemHint}>{local.secret_backend}</span>
              </div>
              <Icon name={local.secret_os_protected ? "lock" : "key"} size={20} />
            </div>
          </div>
        </section>

        <section className={p.section} aria-labelledby="log">
          <div className={s.sectionHead}>
            <h2 id="log" className={p.sectionTitle}>
              Activity
            </h2>
            <div className={p.row}>
              <label className={s.filter}>
                <span className="visually-hidden">Show activity for</span>
                <select value={filter} onChange={(e) => setFilter(e.target.value)} className={s.select}>
                  <option value="">All devices</option>
                  {peers.map((peer) => (
                    <option key={peer.info.id} value={peer.info.id}>
                      {peer.info.name}
                    </option>
                  ))}
                </select>
              </label>
              <Button icon="download" onPress={exportLog} isDisabled={log.length === 0}>
                Export CSV
              </Button>
            </div>
          </div>
          <div className={p.panel}>
            {log.length === 0 ? (
              <div className={p.item}>
                <span className={p.itemHint}>Nothing has happened yet.</span>
              </div>
            ) : (
              <ol className={s.log}>
                {log.map((e) => (
                  <li key={e.id} className={s.logRow}>
                    <time className={s.logTime} dateTime={new Date(e.ts).toISOString()}>
                      {clockTime(e.ts)}
                    </time>
                    <span className={s.logWhat}>
                      {actionLabel(e.action)}
                      {e.device_name ? <span className={s.logWho}> {e.device_name}</span> : null}
                    </span>
                    {e.detail && <span className={s.logDetail}>{e.detail}</span>}
                  </li>
                ))}
              </ol>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
