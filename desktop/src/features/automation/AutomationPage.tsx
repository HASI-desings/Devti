import { useCallback, useEffect, useState } from "react";
import type { AutomationInfo, Rule, RuleAction, RuleTrigger } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { relativeTime } from "../../lib/format";
import { orderedPeers, useStore } from "../../state/store";
import { Button, Field, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Automation.module.css";

const err = (e: unknown) => String(e instanceof Error ? e.message : e);
const newId = () => Math.random().toString(36).slice(2, 10);

const TRIGGERS: { kind: RuleTrigger["kind"]; label: string; make: () => RuleTrigger }[] = [
  {
    kind: "device_connected",
    label: "A device connects",
    make: () => ({ kind: "device_connected", device: "" }),
  },
  {
    kind: "device_disconnected",
    label: "A device disconnects",
    make: () => ({ kind: "device_disconnected", device: "" }),
  },
  {
    kind: "battery_below",
    label: "Battery drops below",
    make: () => ({ kind: "battery_below", device: "", percent: 15 }),
  },
  {
    kind: "accessory_connected",
    label: "An accessory connects",
    make: () => ({ kind: "accessory_connected", name: "" }),
  },
  {
    kind: "proximity_away",
    label: "My phone walks away",
    make: () => ({ kind: "proximity_away", device: "" }),
  },
  {
    kind: "proximity_near",
    label: "My phone comes near",
    make: () => ({ kind: "proximity_near", device: "" }),
  },
  {
    kind: "time",
    label: "At a time",
    make: () => ({ kind: "time", hour: 9, minute: 0, days: [1, 2, 3, 4, 5] }),
  },
  { kind: "app_opened", label: "An app opens here", make: () => ({ kind: "app_opened", app: "" }) },
  {
    kind: "network_changed",
    label: "Wi-Fi network changes",
    make: () => ({ kind: "network_changed", network: "" }),
  },
];

const ACTIONS: { kind: RuleAction["kind"]; label: string; make: () => RuleAction }[] = [
  { kind: "notify", label: "Show a notification", make: () => ({ kind: "notify", title: "", body: "" }) },
  { kind: "lock_computer", label: "Lock this computer", make: () => ({ kind: "lock_computer" }) },
  { kind: "set_dnd", label: "Do Not Disturb", make: () => ({ kind: "set_dnd", on: true }) },
  { kind: "run_command", label: "Run a saved command", make: () => ({ kind: "run_command", command: "" }) },
  { kind: "launch_app", label: "Open an app", make: () => ({ kind: "launch_app", app: "" }) },
  {
    kind: "switch_audio_output",
    label: "Switch audio output",
    make: () => ({ kind: "switch_audio_output", output: "" }),
  },
  {
    kind: "accessory_control",
    label: "Change an accessory setting",
    make: () => ({ kind: "accessory_control", accessory: "", control: "anc", value: "anc" }),
  },
  { kind: "ring_device", label: "Ring a device", make: () => ({ kind: "ring_device", device: "" }) },
];
const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** Hub → Automation: if-this-then-that rules across the ecosystem. */
export function AutomationPage() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [info, setInfo] = useState<AutomationInfo | null>(null);
  const [editing, setEditing] = useState<Rule | null>(null);

  const load = useCallback(
    () => bridge.automation().then(setInfo, (e) => toast(err(e), "danger")),
    [bridge, toast],
  );
  useEffect(() => {
    void load();
  }, [load]);

  const save = (rules: Rule[]) =>
    bridge.saveRules(rules).then(
      () => setInfo((i) => (i ? { ...i, rules } : i)),
      (e) => toast(err(e), "danger"),
    );

  if (!info) return null;
  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Automation</h1>
          <p className={p.intro}>
            When something happens in your ecosystem, Devti does something for you. Rules run on this
            computer.
          </p>
        </header>

        {editing ? (
          <RuleEditor
            rule={editing}
            onCancel={() => setEditing(null)}
            onSave={(r) => {
              const exists = info.rules.some((x) => x.id === r.id);
              void save(exists ? info.rules.map((x) => (x.id === r.id ? r : x)) : [...info.rules, r]);
              setEditing(null);
            }}
          />
        ) : (
          <>
            <div className={s.rules}>
              {info.rules.map((r) => (
                <article key={r.id} className={s.rule} data-on={r.enabled || undefined}>
                  <span className={s.ruleIcon} aria-hidden="true">
                    <Icon name="flow" size={20} />
                  </span>
                  <div className={p.itemText}>
                    <span className={p.itemTitle}>{r.name}</span>
                    <span className={p.itemHint}>
                      When {TRIGGERS.find((t) => t.kind === r.trigger.kind)?.label.toLowerCase()} →{" "}
                      {r.actions
                        .map((a) => ACTIONS.find((x) => x.kind === a.kind)?.label.toLowerCase())
                        .join(", then ")}
                    </span>
                  </div>
                  <div className={s.ruleActions}>
                    <Button size="sm" variant="ghost" onPress={() => setEditing(structuredClone(r))}>
                      Edit
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onPress={() =>
                        bridge.runRuleNow(r.id).then(
                          () => toast("Done.", "ok"),
                          (e) => toast(err(e), "danger"),
                        )
                      }
                    >
                      Try it
                    </Button>
                    <Switch
                      isSelected={r.enabled}
                      onChange={(v) =>
                        void save(info.rules.map((x) => (x.id === r.id ? { ...x, enabled: v } : x)))
                      }
                    >
                      <span className="visually-hidden">{r.name}</span>
                    </Switch>
                  </div>
                </article>
              ))}
            </div>
            <Button
              icon="plus"
              onPress={() =>
                setEditing({
                  id: newId(),
                  name: "",
                  enabled: true,
                  trigger: { kind: "device_connected", device: "" },
                  actions: [{ kind: "notify", title: "", body: "" }],
                  cooldown_s: 60,
                })
              }
            >
              New rule
            </Button>

            <section className={p.section} aria-labelledby="ran">
              <h2 id="ran" className={p.sectionTitle}>
                Recently ran
              </h2>
              {info.log.length === 0 ? (
                <p className={p.sectionIntro}>Nothing yet.</p>
              ) : (
                <div className={p.panel}>
                  {info.log.map((l) => (
                    <div key={`${l.rule}-${l.ts}`} className={p.item}>
                      <div className={p.itemText}>
                        <span className={p.itemTitle}>{l.name}</span>
                        <span className={p.itemHint}>
                          {relativeTime(l.ts)}
                          {l.ok ? "" : ` · ${l.detail}`}
                        </span>
                      </div>
                      <span className={s.result} data-ok={l.ok || undefined}>
                        {l.ok ? "Done" : "Failed"}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </section>
          </>
        )}
      </div>
    </div>
  );
}

function RuleEditor({
  rule,
  onSave,
  onCancel,
}: {
  rule: Rule;
  onSave: (r: Rule) => void;
  onCancel: () => void;
}) {
  const peers = orderedPeers(useStore((st) => st.peers));
  const [r, setR] = useState(rule);
  const setTrigger = (t: RuleTrigger) => setR({ ...r, trigger: t });
  const setAction = (i: number, a: RuleAction) =>
    setR({ ...r, actions: r.actions.map((x, j) => (j === i ? a : x)) });
  const deviceSelect = (value: string, onChange: (v: string) => void, label = "Device") => (
    <label className={s.inline}>
      {label}
      <select className={s.select} value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">Any</option>
        {peers.map((x) => (
          <option key={x.info.id} value={x.info.id}>
            {x.info.name}
          </option>
        ))}
      </select>
    </label>
  );
  const t = r.trigger;
  return (
    <form
      className={s.editor}
      onSubmit={(e) => {
        e.preventDefault();
        if (!r.name.trim()) return;
        onSave(r);
      }}
    >
      <Field
        label="Name"
        value={r.name}
        onChange={(v) => setR({ ...r, name: v })}
        maxLength={80}
        placeholder="Desk mode"
      />

      <fieldset className={s.block}>
        <legend>When</legend>
        <select
          className={s.select}
          aria-label="Trigger"
          value={t.kind}
          onChange={(e) =>
            setTrigger((TRIGGERS.find((x) => x.kind === e.target.value) ?? TRIGGERS[0]!).make())
          }
        >
          {TRIGGERS.map((x) => (
            <option key={x.kind} value={x.kind}>
              {x.label}
            </option>
          ))}
        </select>
        {(t.kind === "device_connected" ||
          t.kind === "device_disconnected" ||
          t.kind === "proximity_away" ||
          t.kind === "proximity_near") &&
          deviceSelect(t.device, (v) => setTrigger({ ...t, device: v }))}
        {t.kind === "battery_below" && (
          <>
            {deviceSelect(t.device, (v) => setTrigger({ ...t, device: v }))}
            <label className={s.inline}>
              Percent
              <input
                className={s.num}
                type="number"
                min={5}
                max={95}
                value={t.percent}
                onChange={(e) => setTrigger({ ...t, percent: Number(e.target.value) })}
              />
            </label>
          </>
        )}
        {t.kind === "accessory_connected" && (
          <Field
            label="Accessory name contains (empty = any)"
            value={t.name}
            onChange={(v) => setTrigger({ ...t, name: v })}
          />
        )}
        {t.kind === "app_opened" && (
          <Field
            label="App (process name)"
            value={t.app}
            onChange={(v) => setTrigger({ ...t, app: v })}
            placeholder="zoom"
          />
        )}
        {t.kind === "network_changed" && (
          <Field
            label="Network name contains (empty = any)"
            value={t.network}
            onChange={(v) => setTrigger({ ...t, network: v })}
          />
        )}
        {t.kind === "time" && (
          <div className={s.timeRow}>
            <label className={s.inline}>
              Time
              <input
                className={s.num}
                type="time"
                value={`${String(t.hour).padStart(2, "0")}:${String(t.minute).padStart(2, "0")}`}
                onChange={(e) => {
                  const [h, m] = e.target.value.split(":").map(Number);
                  setTrigger({ ...t, hour: h ?? 0, minute: m ?? 0 });
                }}
              />
            </label>
            <div className={s.days}>
              {DAYS.map((d, i) => (
                <button
                  type="button"
                  key={d}
                  className={s.day}
                  aria-pressed={t.days.includes(i + 1)}
                  onClick={() =>
                    setTrigger({
                      ...t,
                      days: t.days.includes(i + 1)
                        ? t.days.filter((x) => x !== i + 1)
                        : [...t.days, i + 1].sort(),
                    })
                  }
                >
                  {d}
                </button>
              ))}
            </div>
          </div>
        )}
        {(t.kind === "proximity_away" || t.kind === "proximity_near") && (
          <p className={p.itemHint}>
            Needs the Devti app on the phone with “Let your computer sense when you walk away” on, and
            Bluetooth on both.
          </p>
        )}
      </fieldset>

      <fieldset className={s.block}>
        <legend>Then</legend>
        {r.actions.map((a, i) => (
          <div key={i} className={s.action}>
            <select
              className={s.select}
              aria-label={`Action ${i + 1}`}
              value={a.kind}
              onChange={(e) =>
                setAction(i, (ACTIONS.find((x) => x.kind === e.target.value) ?? ACTIONS[0]!).make())
              }
            >
              {ACTIONS.map((x) => (
                <option key={x.kind} value={x.kind}>
                  {x.label}
                </option>
              ))}
            </select>
            {a.kind === "notify" && (
              <>
                <Field label="Title" value={a.title} onChange={(v) => setAction(i, { ...a, title: v })} />
                <Field label="Text" value={a.body} onChange={(v) => setAction(i, { ...a, body: v })} />
              </>
            )}
            {a.kind === "set_dnd" && (
              <Switch isSelected={a.on} onChange={(v) => setAction(i, { ...a, on: v })}>
                {a.on ? "Turn on" : "Turn off"}
              </Switch>
            )}
            {a.kind === "run_command" && (
              <Field
                label="Saved command id"
                value={a.command}
                onChange={(v) => setAction(i, { ...a, command: v })}
              />
            )}
            {a.kind === "launch_app" && (
              <Field
                label="App id"
                value={a.app}
                onChange={(v) => setAction(i, { ...a, app: v })}
                placeholder="firefox.desktop"
              />
            )}
            {a.kind === "switch_audio_output" && (
              <Field
                label="Output name contains (empty = Bluetooth)"
                value={a.output}
                onChange={(v) => setAction(i, { ...a, output: v })}
              />
            )}
            {a.kind === "accessory_control" && (
              <>
                <Field
                  label="Accessory name"
                  value={a.accessory}
                  onChange={(v) => setAction(i, { ...a, accessory: v })}
                  placeholder="AirPods"
                />
                <Field
                  label="Setting"
                  value={a.control}
                  onChange={(v) => setAction(i, { ...a, control: v })}
                />
                <Field label="Value" value={a.value} onChange={(v) => setAction(i, { ...a, value: v })} />
              </>
            )}
            {a.kind === "ring_device" &&
              deviceSelect(a.device, (v) => setAction(i, { ...a, device: v }), "Which")}
            {r.actions.length > 1 && (
              <Button
                size="sm"
                variant="ghost"
                icon="trash"
                onPress={() => setR({ ...r, actions: r.actions.filter((_, j) => j !== i) })}
              >
                Remove
              </Button>
            )}
          </div>
        ))}
        <Button
          size="sm"
          icon="plus"
          onPress={() => setR({ ...r, actions: [...r.actions, { kind: "notify", title: "", body: "" }] })}
        >
          Add another action
        </Button>
      </fieldset>

      <div className={p.row}>
        <Button type="submit" variant="primary" isDisabled={!r.name.trim()}>
          Save rule
        </Button>
        <Button variant="ghost" onPress={onCancel}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
