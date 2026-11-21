import { type CSSProperties, useEffect, useState } from "react";
import type { BatterySample } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
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
  const until = Date.now();
  const since = until - RANGE_MS[range];

  const devices = [
    {
      id: null as string | null,
      name: local.info.name,
      kind: local.info.kind,
      battery: local.status.battery,
      color: "var(--hub)",
      online: true,
      self: true,
    },
    ...peers.map((x) => ({
      id: x.info.id as string | null,
      name: x.info.name,
      kind: x.info.kind,
      battery: x.status.battery,
      color: `var(--chart-${voices[x.info.id] ?? "iris"})`,
      online: x.online,
      self: false,
    })),
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
            <DeviceCard
              key={d.id ?? "self"}
              d={d}
              since={since}
              until={until}
              threshold={threshold}
              bridge={bridge}
            />
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
  bridge,
}: {
  d: {
    id: string | null;
    name: string;
    kind: import("../../bridge").DeviceKind;
    battery: import("../../bridge").BatteryState | null;
    color: string;
    online: boolean;
    self: boolean;
  };
  since: number;
  until: number;
  threshold: number;
  bridge: ReturnType<typeof useBridge>;
}) {
  const [samples, setSamples] = useState<BatterySample[] | null>(null);
  const [table, setTable] = useState(false);
  const battery = d.battery;

  useEffect(() => {
    let alive = true;
    void bridge.batteryHistory(d.id, since).then((h) => alive && setSamples(h));
    return () => {
      alive = false;
    };
  }, [bridge, d.id, since]);

  return (
    <section
      className={s.card}
      style={{ "--line-color": d.color } as CSSProperties}
      aria-label={`${d.name} battery`}
    >
      <header className={s.cardHead}>
        <Icon name={d.self ? "hub" : deviceIcon(d.kind)} size={22} />
        <div className={s.cardTitle}>
          <h2>{d.name}</h2>
          <span className={s.sub}>
            {d.self ? "This computer" : d.online ? "Connected" : "Offline, last reported level"}
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
