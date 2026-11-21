import { AnimatePresence, motion } from "motion/react";
import type { CSSProperties } from "react";
import type { PeerSnapshot } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { kindLabel, latencyLabel, platformLabel, relativeTime } from "../../lib/format";
import { orderedPeers, useStore } from "../../state/store";
import { BatteryMeter, Button } from "../../ui";
import { EcosystemMap } from "./EcosystemMap";
import s from "./EcosystemPage.module.css";

export function EcosystemPage({ onPair }: { onPair: () => void }) {
  const bridge = useBridge();
  const local = useStore((st) => st.local)!;
  const peersMap = useStore((st) => st.peers);
  const voices = useStore((st) => st.voices);
  const justLinked = useStore((st) => st.justLinked);
  const selected = useStore((st) => st.selected);
  const select = useStore((st) => st.select);
  const nearby = useStore((st) => st.nearby);
  const peers = orderedPeers(peersMap);
  const sel = selected ? peersMap[selected] : undefined;

  return (
    <div className={s.page}>
      <section className={s.stage} aria-label="Ecosystem map">
        <EcosystemMap
          local={local}
          peers={peers}
          voices={voices}
          justLinked={justLinked}
          selected={selected}
          onSelect={select}
          onPair={onPair}
          onResume={() => void bridge.setPaused(false)}
        />
      </section>
      <aside
        className={s.side}
        aria-label={sel ? `${sel.info.name} details` : "Your devices"}
        data-tour="panel"
      >
        <AnimatePresence mode="wait" initial={false}>
          {sel ? (
            <motion.div
              key={sel.info.id}
              initial={{ opacity: 0, x: 16 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -8 }}
              transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            >
              <DeviceSummary peer={sel} voice={voiceVar(voices[sel.info.id] ?? "iris")} />
            </motion.div>
          ) : (
            <motion.div
              key="list"
              initial={{ opacity: 0, x: 16 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -8 }}
              transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            >
              <DeviceList peers={peers} />
              {Object.values(nearby).length > 0 && <Nearby onPair={onPair} />}
            </motion.div>
          )}
        </AnimatePresence>
      </aside>
    </div>
  );
}

function DeviceList({ peers }: { peers: PeerSnapshot[] }) {
  const voices = useStore((st) => st.voices);
  const select = useStore((st) => st.select);
  const local = useStore((st) => st.local)!;
  return (
    <div className={s.block}>
      <h2 className={s.heading}>Your devices</h2>
      <div className={s.self}>
        <Icon name="hub" size={20} />
        <div className={s.rowText}>
          <span className={s.rowName}>{local.info.name}</span>
          <span className={s.rowMeta}>This computer</span>
        </div>
        <BatteryMeter battery={local.status.battery} accent="var(--hub)" />
      </div>
      {peers.length === 0 ? (
        <p className={s.emptyNote}>
          Devices you pair appear here and on the map. Pairing takes about ten seconds.
        </p>
      ) : (
        <ul className={s.list}>
          {peers.map((p) => {
            const color = voiceVar(voices[p.info.id] ?? "iris");
            return (
              <li key={p.info.id}>
                <button
                  type="button"
                  className={s.row}
                  style={{ "--voice": color } as CSSProperties}
                  data-online={p.online || undefined}
                  onClick={() => select(p.info.id)}
                >
                  <Icon name={deviceIcon(p.info.kind)} size={20} dotColor={p.online ? color : undefined} />
                  <span className={s.rowText}>
                    <span className={s.rowName}>{p.info.name}</span>
                    <span className={s.rowMeta}>
                      {p.online ? "Connected" : `Seen ${relativeTime(p.last_seen)}`}
                    </span>
                  </span>
                  {p.online ? <BatteryMeter battery={p.status.battery} accent={color} /> : null}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function Nearby({ onPair }: { onPair: () => void }) {
  const nearbyMap = useStore((st) => st.nearby);
  const nearby = Object.values(nearbyMap);
  return (
    <div className={s.block}>
      <h2 className={s.heading}>On your network</h2>
      <ul className={s.list}>
        {nearby.map((d) => (
          <li key={d.id} className={s.nearbyRow}>
            <Icon name={deviceIcon(d.kind)} size={20} />
            <span className={s.rowText}>
              <span className={s.rowName}>{d.name}</span>
              <span className={s.rowMeta}>
                {d.pairing_open ? "Ready to pair" : `${kindLabel(d.kind)}, not paired`}
              </span>
            </span>
            {d.pairing_open && (
              <Button size="sm" variant="quiet" onPress={onPair}>
                Pair
              </Button>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

function DeviceSummary({ peer, voice }: { peer: PeerSnapshot; voice: string }) {
  const select = useStore((st) => st.select);
  const navigate = useStore((st) => st.navigate);
  const c = peer.connection;
  return (
    <div className={s.block} style={{ "--voice": voice } as CSSProperties}>
      <Button variant="ghost" size="sm" icon="chevronLeft" onPress={() => select(null)}>
        All devices
      </Button>
      <div className={s.summaryHead}>
        <span className={s.summaryIcon} data-online={peer.online || undefined}>
          <Icon name={deviceIcon(peer.info.kind)} size={34} dotColor={peer.online ? voice : undefined} />
        </span>
        <h2 className={s.summaryName}>{peer.info.name}</h2>
        <p className={s.rowMeta}>
          {kindLabel(peer.info.kind)} with {peer.info.os_version || platformLabel(peer.info.platform)}
        </p>
      </div>
      <dl className={s.facts}>
        <div>
          <dt>Status</dt>
          <dd>{peer.online ? "Connected" : `Offline, seen ${relativeTime(peer.last_seen)}`}</dd>
        </div>
        {peer.status.battery && (
          <div>
            <dt>Battery</dt>
            <dd>
              <BatteryMeter battery={peer.status.battery} accent={voice} />
            </dd>
          </div>
        )}
        {c && (
          <div>
            <dt>Link</dt>
            <dd>
              {latencyLabel(c.rtt_ms)}, <span className="num">{c.rtt_ms} ms</span>
            </dd>
          </div>
        )}
        <div>
          <dt>Access</dt>
          <dd>{peer.role === "trusted" ? "Trusted" : "Restricted"}</dd>
        </div>
      </dl>
      <div className={s.summaryActions}>
        <Button variant="primary" icon="chevronRight" onPress={() => navigate("device", peer.info.id)}>
          Open device
        </Button>
      </div>
    </div>
  );
}
