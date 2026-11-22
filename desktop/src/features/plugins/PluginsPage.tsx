import { useCallback, useEffect, useState } from "react";
import type { PluginInfo } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button, Sheet, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Plugins.module.css";

const errText = (e: unknown) => String(e instanceof Error ? e.message : e);

/** Phase 7: plugins — small sandboxed programs that add to Devti. */
export function PluginsPage() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [list, setList] = useState<PluginInfo[] | null>(null);
  const [offer, setOffer] = useState<{ path: string; info: PluginInfo } | null>(null);
  const [outputs, setOutputs] = useState<Record<string, { ok: boolean; text: string }>>({});

  const refresh = useCallback(() => void bridge.plugins().then(setList), [bridge]);
  useEffect(refresh, [refresh]);

  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      toast(errText(e), "danger");
    }
    refresh();
  };

  const choose = async () => {
    const path = await bridge.pickFolder("Choose a plugin folder (with devti-plugin.json and plugin.wasm)");
    if (!path) return;
    try {
      setOffer({ path, info: await bridge.pluginInspect(path) });
    } catch (e) {
      toast(errText(e), "danger");
    }
  };

  const install = async () => {
    if (!offer) return;
    const name = offer.info.manifest.name;
    setOffer(null);
    await act(async () => {
      await bridge.pluginInstall(offer.path);
      toast(`${name} is installed and running.`, "ok");
    });
  };

  const run = async (id: string, command: string) => {
    const key = `${id}:${command}`;
    try {
      setOutputs((o) => ({ ...o, [key]: { ok: true, text: "Running…" } }));
      const text = await bridge.pluginRun(id, command);
      setOutputs((o) => ({ ...o, [key]: { ok: true, text: text || "Done." } }));
    } catch (e) {
      setOutputs((o) => ({ ...o, [key]: { ok: false, text: errText(e) } }));
    }
  };

  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Plugins</h1>
          <p className={p.intro}>
            Small programs that add to Devti. Each runs sealed off in its own sandbox — no files, no other
            apps — and can only do what you allowed when you added it. Their commands also appear on your
            phone's remote and deck.
          </p>
        </header>
        <div className={p.row}>
          <Button icon="plus" onPress={() => void choose()}>
            Add a plugin
          </Button>
          <span className={s.hint}>
            Only add plugins from people you trust. To write one, see docs/PLUGINS.md in the Devti source.
          </span>
        </div>

        {list && list.length === 0 && (
          <div className={s.empty}>
            <Icon name="plugin" size={28} />
            <p>
              No plugins yet. The Devti source includes an example — <strong>Link cleaner</strong>, which
              removes tracking from links you copy — in <code>plugins/link-cleaner</code>.
            </p>
          </div>
        )}

        <ul className={s.list}>
          {list?.map((pl) => {
            const m = pl.manifest;
            const status = pl.problem
              ? { tone: "danger", text: pl.problem }
              : pl.needs_approval
                ? {
                    tone: "warn",
                    text: "This version asks for more than you approved. Turn it on to approve.",
                  }
                : pl.running
                  ? { tone: "ok", text: "Running" }
                  : { tone: "dim", text: "Off" };
            return (
              <li key={m.id} className={s.card} data-on={pl.running || undefined}>
                <div className={s.top}>
                  <span className={s.badge} aria-hidden="true">
                    <Icon name="plugin" size={22} />
                  </span>
                  <div className={s.title}>
                    <h2>
                      {m.name} <span className={s.version}>{m.version}</span>
                    </h2>
                    {m.author && <span className={s.hint}>by {m.author}</span>}
                  </div>
                  <Switch
                    isSelected={pl.enabled && !pl.needs_approval}
                    onChange={(v) => void act(() => bridge.pluginSetEnabled(m.id, v))}
                  >
                    <span className="visually-hidden">Turn {m.name} on</span>
                  </Switch>
                </div>
                {m.description && <p className={s.description}>{m.description}</p>}
                <p
                  className={s.status}
                  data-tone={status.tone}
                  role={status.tone === "danger" ? "alert" : undefined}
                >
                  {status.text}
                </p>
                {m.commands.length > 0 && (
                  <div className={s.commands}>
                    {m.commands.map((c) => {
                      const out = outputs[`${m.id}:${c.id}`];
                      return (
                        <div key={c.id} className={s.command}>
                          <Button variant="quiet" size="sm" icon="play" onPress={() => void run(m.id, c.id)}>
                            {c.name}
                          </Button>
                          {out && (
                            <span className={s.output} data-ok={out.ok || undefined} aria-live="polite">
                              {out.text}
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
                <details className={s.details}>
                  <summary>What it may do ({pl.permissions.length})</summary>
                  <ul className={s.perms}>
                    {pl.permissions.length === 0 && <li>Nothing beyond its own commands.</li>}
                    {pl.permissions.map(([key, text]) => (
                      <li key={key}>{text}</li>
                    ))}
                  </ul>
                </details>
                {pl.log.length > 0 && (
                  <details className={s.details}>
                    <summary>Recent activity</summary>
                    <ol className={s.log}>
                      {pl.log
                        .slice(-12)
                        .reverse()
                        .map((line, i) => (
                          <li key={i}>{line}</li>
                        ))}
                    </ol>
                  </details>
                )}
                <div className={s.footer}>
                  <Button
                    variant="ghost"
                    size="sm"
                    icon="trash"
                    onPress={() => void act(() => bridge.pluginRemove(m.id))}
                  >
                    Remove
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      </div>

      <Sheet isOpen={!!offer} onClose={() => setOffer(null)} label="Add a plugin" narrow>
        {offer && (
          <div className={s.consent}>
            <span className={s.badge} aria-hidden="true">
              <Icon name="plugin" size={26} />
            </span>
            <h2>
              Add {offer.info.manifest.name} {offer.info.manifest.version}?
            </h2>
            {offer.info.manifest.author && <p className={s.hint}>by {offer.info.manifest.author}</p>}
            {offer.info.manifest.description && <p>{offer.info.manifest.description}</p>}
            <h3>It will be able to</h3>
            <ul className={s.perms}>
              {offer.info.permissions.length === 0 && <li>Run its own commands — nothing else.</li>}
              {offer.info.permissions.map(([key, text]) => (
                <li key={key}>{text}</li>
              ))}
            </ul>
            <p className={s.hint}>
              It can't read your files, start programs or reach any website not listed above. You can turn it
              off or remove it at any time.
            </p>
            <div className={p.row}>
              <Button onPress={() => void install()}>Add and turn on</Button>
              <Button variant="ghost" onPress={() => setOffer(null)}>
                Cancel
              </Button>
            </div>
          </div>
        )}
      </Sheet>
    </div>
  );
}
