import type { CSSProperties } from "react";
import { useEffect, useState } from "react";
import type { Accessory, AccessoryControl, AccessoryList } from "../../bridge";
import { Icon, type IconName } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { relativeTime } from "../../lib/format";
import { useStore } from "../../state/store";
import { Button, Segmented, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Accessories.module.css";
import { SoundCard } from "./SoundCard";

const KIND_ICON: Record<Accessory["kind"], IconName> = {
  headphones: "headphones",
  speaker: "speaker",
  keyboard: "keyboard",
  mouse: "mouse",
  gamepad: "gamepad",
  watch: "watch",
  other: "bluetooth",
};
const COMPONENT_LABEL: Record<string, string> = {
  left: "Left",
  right: "Right",
  case: "Case",
  battery: "Battery",
};
const err = (e: unknown) => String(e instanceof Error ? e.message : e);

/** Hub → Accessories: every Bluetooth accessory with a control panel built from its profile. */
export function AccessoriesPage() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [data, setData] = useState<AccessoryList>({ status: "", list: [] });
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    void bridge.accessories().then(setData, (e) => toast(err(e), "danger"));
    return bridge.onAccessories(setData);
  }, [bridge, toast]);

  const list = [...data.list].sort(
    (a, b) => Number(b.connected) - Number(a.connected) || a.name.localeCompare(b.name),
  );
  const current = list.find((a) => a.address === selected) ?? list[0];

  return (
    <div className={p.page}>
      <div className={s.wrap}>
        <header className={p.hero}>
          <h1 className={p.title}>Accessories</h1>
          <p className={p.intro}>
            Earbuds, headphones, speakers, keyboards and mice paired with this computer. Devti recognises each
            one and builds its controls for you.
          </p>
          {data.status && <p className={p.note}>{data.status}</p>}
        </header>
        <SoundCard accessories={data.list} />
        {list.length === 0 ? (
          <div className={s.empty}>
            <Icon name="bluetooth" size={36} />
            <p>
              No accessories yet. Pair one in your system's Bluetooth settings — it shows up here with its
              battery and controls.
            </p>
          </div>
        ) : (
          <div className={s.layout}>
            <ul className={s.list} aria-label="Accessories">
              {list.map((a) => (
                <li key={a.address}>
                  <button
                    type="button"
                    className={s.item}
                    aria-current={current?.address === a.address || undefined}
                    data-connected={a.connected || undefined}
                    onClick={() => setSelected(a.address)}
                  >
                    <span className={s.itemIcon}>
                      <Icon
                        name={KIND_ICON[a.kind]}
                        size={22}
                        dotColor={a.connected ? "var(--ok)" : undefined}
                      />
                    </span>
                    <span className={s.itemText}>
                      <span className={s.itemName}>{a.name}</span>
                      <span className={s.itemMeta}>
                        {a.connected
                          ? "Connected"
                          : a.last_connected
                            ? `Seen ${relativeTime(a.last_connected)}`
                            : "Not connected"}
                      </span>
                    </span>
                    <MiniBattery battery={a.battery} />
                  </button>
                </li>
              ))}
            </ul>
            {current && <Detail key={current.address} a={current} />}
          </div>
        )}
      </div>
    </div>
  );
}

function MiniBattery({ battery }: { battery: Accessory["battery"] }) {
  const parts = Object.entries(battery).filter(([k]) => k !== "case");
  if (!parts.length) return null;
  const low = Math.min(...parts.map(([, v]) => v.percent));
  return (
    <span className={s.mini} data-low={low <= 20 || undefined}>
      <span className="num">{low}%</span>
    </span>
  );
}

function Detail({ a }: { a: Accessory }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const act = (fn: () => Promise<unknown>) => fn().catch((e) => toast(err(e), "danger"));
  const set = (control: string, value: string) =>
    act(() => bridge.setAccessoryControl(a.address, control, value));
  const live = a.connected && a.session === "connected";

  return (
    <section className={s.detail} aria-labelledby="acc-name">
      <header className={s.detailHead}>
        <span className={s.bigIcon} data-connected={a.connected || undefined}>
          <Icon name={KIND_ICON[a.kind]} size={40} />
        </span>
        <div className={p.itemText}>
          <h2 id="acc-name" className={s.name}>
            {a.name}
          </h2>
          <span className={p.itemHint}>
            {a.connected ? "Connected to this computer" : "Not connected"}
            {a.rssi != null && ` · signal ${a.rssi} dBm`}
          </span>
        </div>
        <Button size="sm" onPress={() => act(() => bridge.connectAccessory(a.address, !a.connected))}>
          {a.connected ? "Disconnect" : "Connect"}
        </Button>
      </header>

      {Object.keys(a.battery).length > 0 && (
        <div className={s.batteries}>
          {Object.entries(a.battery)
            .sort(
              ([x], [y]) =>
                ["left", "right", "battery", "case"].indexOf(x) -
                ["left", "right", "battery", "case"].indexOf(y),
            )
            .map(([k, v]) => (
              <div
                key={k}
                className={s.cell}
                style={{ "--level": v.percent } as CSSProperties}
                data-low={v.percent <= 20 || undefined}
              >
                <svg viewBox="0 0 48 48" className={s.ring} aria-hidden="true">
                  <circle cx="24" cy="24" r="20" className={s.ringTrack} />
                  <circle
                    cx="24"
                    cy="24"
                    r="20"
                    className={s.ringFill}
                    pathLength={100}
                    strokeDasharray={`${v.percent} 100`}
                  />
                </svg>
                <span className={s.cellValue}>
                  <b className="num">{v.percent}%</b>
                  {v.charging && <Icon name="bolt" size={12} />}
                </span>
                <span className={s.cellLabel}>{COMPONENT_LABEL[k] ?? k}</span>
              </div>
            ))}
        </div>
      )}
      {(a.values.ear_primary || a.values.ear_secondary) && (
        <p className={p.itemHint}>
          In-ear: {a.values.ear_primary ?? "?"} / {a.values.ear_secondary ?? "?"}
        </p>
      )}

      <div className={s.profile}>
        <div className={s.profileHead}>
          <span className={p.itemTitle}>{a.profile.name}</span>
          {a.profile.verified ? (
            <span className={s.badge} data-ok="">
              Verified
            </span>
          ) : (
            <span className={s.badge}>Untested on real hardware</span>
          )}
        </div>
        <span className={p.itemHint}>{a.profile.notes}</span>
        {a.profile.has_channel && !live && (
          <span className={s.session}>
            {a.connected
              ? a.session === "connecting"
                ? "Opening its control channel…"
                : a.session
              : "Connect it to change its settings."}
          </span>
        )}
      </div>

      {a.profile.controls.length > 0 && (
        <div className={s.controls} aria-disabled={!live || undefined}>
          {a.profile.controls.map((c) => (
            <ControlRow
              key={c.id}
              c={c}
              value={a.values[c.id]}
              disabled={!live}
              onChange={(v) => set(c.id, v)}
            />
          ))}
        </div>
      )}

      <div className={p.panel}>
        <div className={p.item}>
          <div className={p.itemText}>
            <span className={p.itemTitle}>Remember settings for this accessory</span>
            <span className={p.itemHint}>Restored automatically when it reconnects.</span>
          </div>
          <Switch
            isSelected={a.remember}
            onChange={(v) => act(() => bridge.setAccessoryRemember(a.address, v))}
          >
            <span className="visually-hidden">Remember settings</span>
          </Switch>
        </div>
        <div className={p.item}>
          <div className={p.itemText}>
            <span className={p.itemTitle}>Health</span>
            <span className={p.itemHint}>
              {a.drops} dropped connection{a.drops === 1 ? "" : "s"}
              {a.last_connected ? ` · last connected ${relativeTime(a.last_connected)}` : ""} ·{" "}
              {a.services.length} services
            </span>
          </div>
        </div>
        {a.profile.id === "generic" && (
          <div className={p.item}>
            <div className={p.itemText}>
              <span className={p.itemTitle}>Help support this device</span>
              <span className={p.itemHint}>
                Saves a draft profile with only its name and Bluetooth layout. Nothing is sent anywhere —
                submit it on GitHub if you like.
              </span>
            </div>
            <Button
              size="sm"
              icon="download"
              onPress={() =>
                act(async () => {
                  const path = await bridge.helpSupportAccessory(a.address);
                  toast(`Draft profile saved: ${path}`, "ok");
                })
              }
            >
              Save draft
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}

function ControlRow({
  c,
  value,
  disabled,
  onChange,
}: {
  c: AccessoryControl;
  value: string | undefined;
  disabled: boolean;
  onChange: (v: string) => void;
}) {
  return (
    <div className={s.control}>
      <div className={p.itemText}>
        <span className={p.itemTitle}>{c.label}</span>
        {c.hint && <span className={p.itemHint}>{c.hint}</span>}
      </div>
      {c.type === "choice" && (
        <Segmented<string>
          label={c.label}
          value={value ?? ""}
          onChange={(v) => !disabled && onChange(v)}
          options={c.options.map((o) => ({ value: o.value, label: o.label }))}
        />
      )}
      {c.type === "toggle" && (
        <Switch
          isSelected={value === "on"}
          isDisabled={disabled}
          onChange={(v) => onChange(v ? "on" : "off")}
        >
          <span className="visually-hidden">{c.label}</span>
        </Switch>
      )}
      {c.type === "slider" && <Slider c={c} value={value} disabled={disabled} onChange={onChange} />}
      {c.type === "equalizer" && <Equalizer c={c} value={value} disabled={disabled} onChange={onChange} />}
      {c.type === "action" && (
        <Button size="sm" isDisabled={disabled} onPress={() => onChange("go")}>
          {c.label}
        </Button>
      )}
    </div>
  );
}

function Slider({
  c,
  value,
  disabled,
  onChange,
}: {
  c: Extract<AccessoryControl, { type: "slider" }>;
  value: string | undefined;
  disabled: boolean;
  onChange: (v: string) => void;
}) {
  const [v, setV] = useState(Number(value ?? c.min));
  return (
    <label className={s.slider}>
      <input
        type="range"
        min={c.min}
        max={c.max}
        step={c.step || 1}
        value={v}
        disabled={disabled}
        onChange={(e) => setV(Number(e.target.value))}
        onPointerUp={() => onChange(String(v))}
        onKeyUp={() => onChange(String(v))}
        aria-label={c.label}
      />
      <span className="num">{v}</span>
    </label>
  );
}

function Equalizer({
  c,
  value,
  disabled,
  onChange,
}: {
  c: Extract<AccessoryControl, { type: "equalizer" }>;
  value: string | undefined;
  disabled: boolean;
  onChange: (v: string) => void;
}) {
  const initial = (value ?? "").split(",").map(Number);
  const [bands, setBands] = useState(
    c.bands.map((_, i) => (Number.isFinite(initial[i]) ? (initial[i] ?? 0) : 0)),
  );
  const commit = (next: number[]) => onChange(next.join(","));
  return (
    <fieldset className={s.eq}>
      <legend className="visually-hidden">{c.label}</legend>
      <svg
        className={s.eqCurve}
        viewBox={`0 0 ${(c.bands.length - 1) * 10} 20`}
        preserveAspectRatio="none"
        aria-hidden="true"
      >
        <polyline
          points={bands.map((b, i) => `${i * 10},${10 - (b / (c.max || 1)) * 9}`).join(" ")}
          fill="none"
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      <div className={s.eqBands}>
        {c.bands.map((name, i) => (
          <label key={name} className={s.eqBand}>
            <input
              type="range"
              min={c.min}
              max={c.max}
              value={bands[i] ?? 0}
              disabled={disabled}
              aria-label={`${name} band`}
              onChange={(e) => {
                const next = [...bands];
                next[i] = Number(e.target.value);
                setBands(next);
              }}
              onPointerUp={() => commit(bands)}
              onKeyUp={() => commit(bands)}
            />
            <span className="num">{(bands[i] ?? 0) > 0 ? `+${bands[i]}` : bands[i]}</span>
            <span className={s.eqName}>{name}</span>
          </label>
        ))}
      </div>
      <Button
        size="sm"
        variant="ghost"
        isDisabled={disabled}
        onPress={() => {
          const flat = c.bands.map(() => 0);
          setBands(flat);
          commit(flat);
        }}
      >
        Reset to flat
      </Button>
    </fieldset>
  );
}
