import { type CSSProperties, useEffect, useState } from "react";
import type { Accessory, BatterySample } from "../../bridge";
import { deviceIcon, Icon, type IconName } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { orderedPeers, useStore } from "../../state/store";
import { BatteryMeter, Segmented } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Battery.module.css";
import { BatteryChart } from "./BatteryChart";

type Range = "day" | "week";
const RANGE_MS: Record<Range, number> = { day: 24 * 3600_000, week: 7 * 24 * 3600_000 };

export function BatteryPage() {
  const bridge = useBridge();
  const local = useStore((st) => st.local)!;
  const peers = orderedPeers(useStore((st) => st.peers));
  const voices = useStore((st) => st.voices);
  const threshold = useStore((st) => st.lowBatteryThreshold);
  const [range, setRange] = useState<Range>("day");
  const [accessories, setAccessories] = useState<Accessory[]>([]);
  const until = Date.now();
  const since = until - RANGE_MS[range];

  useEffect(() => {
    void bridge.accessories().then(
      (a) => setAccessories(a.list),
      () => {},
    );
    return bridge.onAccessories((a) => setAccessories(a.list));
  }, [bridge]);

  const devices = [
    {
      id: null as string | null,
      name: local.info.name,
      kind: local.info.kind,
      battery: local.status.battery,
      color: "var(--hub)",
      online: true,
      self: true,
      icon: "hub" as IconName,
      load: (from: number) => bridge.batteryHistory(null, from),
    },
    ...peers.map((x) => ({
      id: x.info.id as string | null,
      name: x.info.name,
      kind: x.info.kind,
      battery: x.status.battery,
      color: `var(--chart-${voices[x.info.id] ?? "iris"})`,
      online: x.online,
      self: false,
      icon: deviceIcon(x.info.kind),
      load: (from: number) => bridge.batteryHistory(x.info.id, from),
    })),
    // Accessories: the lowest bud (or the device's one battery) — the one that runs out first.
    ...accessories
      .filter((a) => Object.keys(a.battery).length > 0)
      .map((a) => {
        const parts = Object.entries(a.battery).filter(([k]) => k !== "case");
        const low = parts.reduce(
          (m, [, v]) => (v.percent < m.percent ? v : m),
          parts[0]?.[1] ?? { percent: 0, charging: false },
        );
        return {
          id: `bt:${a.address}` as string | null,
          name: a.name,
          kind: "accessory" as import("../../bridge").DeviceKind,
          battery: { percent: low.percent, charging: low.charging },
          color: "var(--chart-mint)",
          online: a.connected,
          self: false,
          icon: (a.kind === "headphones"
            ? "headphones"
            : a.kind === "speaker"
              ? "speaker"
              : "bluetooth") as IconName,
          load: (from: number) => bridge.accessoryBatteryHistory(a.address, from),
        };
      }),
  ];

  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Battery</h1>
          <p className={p.intro}>
            Every device's battery, and how it has changed. Devti warns you when one runs low.
          </p>
        </header>
        <div className={s.controls}>
          <Segmented<Range>
            label="Time range"
            value={range}
            onChange={setRange}
            options={[
              { value: "day", label: "Last 24 hours" },
              { value: "week", label: "Last 7 days" },
            ]}
          />
          <p className={s.key}>
            <span className={s.keySwatch} aria-hidden /> Shaded areas: charging
          </p>
        </div>
        <div className={s.cards}>
          {devices.map((d) => (
            <DeviceCard key={d.id ?? "self"} d={d} since={since} until={until} threshold={threshold} />
          ))}
        </div>
      </div>
    </div>
  );
}

function DeviceCard({
  d,
  since,
  until,
  threshold,
}: {
  d: {
    id: string | null;
    name: string;
    kind: import("../../bridge").DeviceKind;
    battery: import("../../bridge").BatteryState | null;
    color: string;
    online: boolean;
    self: boolean;
    icon: IconName;
    load: (since: number) => Promise<BatterySample[]>;
  };
  since: number;
  until: number;
  threshold: number;
}) {
  const [samples, setSamples] = useState<BatterySample[] | null>(null);
  const [table, setTable] = useState(false);
  const battery = d.battery;

  // biome-ignore lint/correctness/useExhaustiveDependencies: `load` is recreated every render; the id identifies it
  useEffect(() => {
    let alive = true;
    void d.load(since).then((h) => alive && setSamples(h));
    return () => {
      alive = false;
    };
  }, [d.id, since]);

  return (
    <section
      className={s.card}
      style={{ "--line-color": d.color } as CSSProperties}
      aria-label={`${d.name} battery`}
    >
      <header className={s.cardHead}>
        <Icon name={d.icon} size={22} />
        <div className={s.cardTitle}>
          <h2>{d.name}</h2>
          <span className={s.sub}>
            {d.self
              ? "This computer"
              : d.kind === "accessory"
                ? d.online
                  ? "Accessory, connected"
                  : "Accessory, last reported level"
                : d.online
                  ? "Connected"
                  : "Offline, last reported level"}
          </span>
        </div>
        <BatteryMeter battery={battery} accent={d.color} />
      </header>
      {samples === null ? (
        <div className={s.placeholder} aria-busy="true" />
      ) : samples.length === 0 ? (
        <p className={s.none}>
          {battery
            ? "History starts building from now."
            : "This device has no battery, or doesn't report one."}
        </p>
      ) : table ? (
        <div className={s.tableWrap}>
          <table className={s.table}>
            <thead>
              <tr>
                <th scope="col">Time</th>
                <th scope="col">Level</th>
                <th scope="col">Charging</th>
              </tr>
            </thead>
            <tbody>
              {[...samples]
                .reverse()
                .slice(0, 200)
                .map((x) => (
                  <tr key={x.ts}>
                    <td>
                      {new Date(x.ts).toLocaleString(undefined, {
                        weekday: "short",
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </td>
                    <td className="num">{x.percent}%</td>
                    <td>{x.charging ? "Yes" : "No"}</td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      ) : (
        <BatteryChart
          samples={samples}
          since={since}
          until={until}
          color={d.color}
          threshold={threshold}
          label={d.name}
        />
      )}
      {samples && samples.length > 0 && (
        <button type="button" className={s.toggle} onClick={() => setTable((t) => !t)}>
          {table ? "Show as chart" : "Show as table"}
        </button>
      )}
    </section>
  );
}
