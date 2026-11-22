import type { CSSProperties } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { DeckActionKind, DeckButtonDef, DeckPageDef, RemoteSettings, SavedCommand } from "../../bridge";
import { Icon, type IconName } from "../../design/icons";
import { VOICES, voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button, Field, Segmented, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Control.module.css";

type Tab = "deck" | "commands" | "api";

const ACTION_LABEL: Record<DeckActionKind, string> = {
  hotkey: "Key combination",
  media: "Media key",
  launch: "Open an app",
  command: "Run a saved command",
  text: "Type text",
  url: "Open a web page",
  obs_scene: "Switch OBS scene",
};

const MEDIA = [
  ["play_pause", "Play / pause"],
  ["next", "Next track"],
  ["previous", "Previous track"],
  ["volume_up", "Volume up"],
  ["volume_down", "Volume down"],
  ["mute", "Mute"],
  ["stop", "Stop"],
] as const;

export const DECK_ICONS: IconName[] = [
  "play",
  "next",
  "previous",
  "mute",
  "volume",
  "windows",
  "desktop",
  "copy",
  "paste",
  "terminal",
  "rocket",
  "camera",
  "record",
  "lock",
  "power",
  "sparkle",
  "monitor",
  "keyboard",
  "folder",
  "bell",
];

const newId = () => Math.random().toString(36).slice(2, 10);
const err = (e: unknown) => String(e instanceof Error ? e.message : e);

/** Hub → Control: the shortcuts deck, saved commands and the local API. */
export function ControlPage() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [tab, setTab] = useState<Tab>("deck");
  const [settings, setSettings] = useState<RemoteSettings | null>(null);

  const reload = useCallback(async () => {
    try {
      setSettings(await bridge.remoteSettings());
    } catch (e) {
      toast(err(e), "danger");
    }
  }, [bridge, toast]);
  useEffect(() => {
    void reload();
  }, [reload]);

  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Control</h1>
          <p className={p.intro}>
            Your phone is a touchpad, keyboard and remote for this computer — and a deck of buttons you design
            here. Only what you set up on this page can be triggered from other devices.
          </p>
        </header>
        <Segmented<Tab>
          label="Show"
          value={tab}
          onChange={setTab}
          options={[
            { value: "deck", label: "Shortcuts deck", icon: "deck" },
            { value: "commands", label: "Saved commands", icon: "terminal" },
            { value: "api", label: "Local API and CLI", icon: "key" },
          ]}
        />
        {!settings ? (
          <p className={p.sectionIntro}>Loading…</p>
        ) : tab === "deck" ? (
          <DeckEditor settings={settings} onSaved={reload} />
        ) : tab === "commands" ? (
          <CommandsEditor settings={settings} onSaved={reload} />
        ) : (
          <ApiPanel settings={settings} onChange={reload} />
        )}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------- deck

function DeckEditor({ settings, onSaved }: { settings: RemoteSettings; onSaved: () => void }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [pages, setPages] = useState<DeckPageDef[]>(settings.deck);
  const [pageIx, setPageIx] = useState(0);
  const [sel, setSel] = useState<string | null>(null);
  const saveTimer = useRef<number | undefined>(undefined);

  const commit = (next: DeckPageDef[]) => {
    setPages(next);
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      bridge.saveDeck(next).then(onSaved, (e) => toast(err(e), "danger"));
    }, 400);
  };

  const page = pages[pageIx] ?? pages[0];
  const selected = page?.buttons.find((b) => b.id === sel) ?? null;
  const updateButton = (b: DeckButtonDef) =>
    commit(
      pages.map((pg, i) =>
        i === pageIx ? { ...pg, buttons: pg.buttons.map((x) => (x.id === b.id ? b : x)) } : pg,
      ),
    );

  const addButton = () => {
    const b: DeckButtonDef = {
      id: newId(),
      label: "New button",
      icon: "sparkle",
      voice: VOICES[(page?.buttons.length ?? 0) % VOICES.length] ?? "iris",
      action: { kind: "hotkey", value: "ctrl+s" },
    };
    commit(pages.map((pg, i) => (i === pageIx ? { ...pg, buttons: [...pg.buttons, b] } : pg)));
    setSel(b.id);
  };

  return (
    <section className={p.section} aria-labelledby="deck-h">
      <h2 id="deck-h" className="visually-hidden">
        Shortcuts deck
      </h2>
      <p className={p.sectionIntro}>
        Open <strong>Deck</strong> in the Devti app on your phone or tablet to use these buttons. Each one
        runs here, on this computer.
      </p>
      <div className={s.pages} role="tablist" aria-label="Deck pages">
        {pages.map((pg, i) => (
          <button
            type="button"
            role="tab"
            aria-selected={i === pageIx}
            key={`${pg.name}-${i}`}
            className={s.pageTab}
            onClick={() => {
              setPageIx(i);
              setSel(null);
            }}
          >
            {pg.name}
          </button>
        ))}
        {pages.length < 12 && (
          <button
            type="button"
            className={s.pageAdd}
            onClick={() => {
              commit([...pages, { name: `Page ${pages.length + 1}`, buttons: [] }]);
              setPageIx(pages.length);
              setSel(null);
            }}
          >
            <Icon name="plus" size={16} /> Page
          </button>
        )}
      </div>

      <div className={s.deckLayout}>
        <fieldset className={s.deckGrid}>
          <legend className="visually-hidden">{`${page?.name ?? "Deck"} buttons`}</legend>
          {page?.buttons.map((b) => (
            <button
              type="button"
              key={b.id}
              className={s.deckButton}
              aria-pressed={b.id === sel}
              style={{ "--voice": voiceVar((b.voice as never) || "iris") } as CSSProperties}
              onClick={() => setSel(b.id)}
            >
              <Icon
                name={(DECK_ICONS.includes(b.icon as IconName) ? b.icon : "sparkle") as IconName}
                size={28}
              />
              <span>{b.label}</span>
            </button>
          ))}
          {(page?.buttons.length ?? 0) < 48 && (
            <button type="button" className={s.deckAdd} onClick={addButton}>
              <Icon name="plus" size={24} />
              <span>Add button</span>
            </button>
          )}
        </fieldset>

        <aside className={s.inspector} aria-label="Button settings">
          {selected ? (
            <ButtonInspector
              key={selected.id}
              button={selected}
              settings={settings}
              onChange={updateButton}
              onDelete={() => {
                commit(
                  pages.map((pg, i) =>
                    i === pageIx ? { ...pg, buttons: pg.buttons.filter((x) => x.id !== selected.id) } : pg,
                  ),
                );
                setSel(null);
              }}
            />
          ) : (
            <div className={s.inspectorEmpty}>
              <Icon name="deck" size={32} />
              <p>Select a button to change what it does, or add a new one.</p>
              {page && (
                <>
                  <Field
                    label="Page name"
                    value={page.name}
                    maxLength={40}
                    onChange={(v) => commit(pages.map((pg, i) => (i === pageIx ? { ...pg, name: v } : pg)))}
                  />
                  {pages.length > 1 && (
                    <Button
                      variant="ghost"
                      icon="trash"
                      onPress={() => {
                        commit(pages.filter((_, i) => i !== pageIx));
                        setPageIx(0);
                      }}
                    >
                      Delete this page
                    </Button>
                  )}
                </>
              )}
            </div>
          )}
        </aside>
      </div>
    </section>
  );
}

function ButtonInspector({
  button,
  settings,
  onChange,
  onDelete,
}: {
  button: DeckButtonDef;
  settings: RemoteSettings;
  onChange: (b: DeckButtonDef) => void;
  onDelete: () => void;
}) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const set = (patch: Partial<DeckButtonDef>) => onChange({ ...button, ...patch });
  const setAction = (patch: Partial<DeckButtonDef["action"]>) =>
    onChange({ ...button, action: { ...button.action, ...patch } });
  const kind = button.action.kind;

  return (
    <div className={s.inspectorBody}>
      <Field label="Label" value={button.label} maxLength={40} onChange={(v) => set({ label: v })} />
      <fieldset className={s.fieldset}>
        <legend>Colour</legend>
        <div className={s.swatches}>
          {VOICES.map((v) => (
            <button
              type="button"
              key={v}
              className={s.swatch}
              aria-pressed={button.voice === v}
              style={{ "--voice": voiceVar(v) } as CSSProperties}
              onClick={() => set({ voice: v })}
            >
              <span className="visually-hidden">{v}</span>
            </button>
          ))}
        </div>
      </fieldset>
      <fieldset className={s.fieldset}>
        <legend>Icon</legend>
        <div className={s.icons}>
          {DECK_ICONS.map((ic) => (
            <button
              type="button"
              key={ic}
              className={s.iconPick}
              aria-pressed={button.icon === ic}
              onClick={() => set({ icon: ic })}
            >
              <Icon name={ic} size={20} />
              <span className="visually-hidden">{ic}</span>
            </button>
          ))}
        </div>
      </fieldset>
      <label className={s.selectLabel}>
        When pressed
        <select
          className={s.select}
          value={kind}
          onChange={(e) => setAction({ kind: e.target.value as DeckActionKind, value: "" })}
        >
          {(Object.keys(ACTION_LABEL) as DeckActionKind[]).map((k) => (
            <option key={k} value={k}>
              {ACTION_LABEL[k]}
            </option>
          ))}
        </select>
      </label>
      {kind === "media" && (
        <select
          className={s.select}
          aria-label="Media key"
          value={button.action.value}
          onChange={(e) => setAction({ value: e.target.value })}
        >
          <option value="">Choose…</option>
          {MEDIA.map(([v, l]) => (
            <option key={v} value={v}>
              {l}
            </option>
          ))}
        </select>
      )}
      {kind === "launch" && (
        <select
          className={s.select}
          aria-label="App"
          value={button.action.value}
          onChange={(e) => setAction({ value: e.target.value })}
        >
          <option value="">Choose an app…</option>
          {settings.apps.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      )}
      {kind === "command" &&
        (settings.commands.length ? (
          <select
            className={s.select}
            aria-label="Saved command"
            value={button.action.value}
            onChange={(e) => setAction({ value: e.target.value })}
          >
            <option value="">Choose a command…</option>
            {settings.commands.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        ) : (
          <p className={p.itemHint}>Add a saved command first, in the Saved commands tab.</p>
        ))}
      {(kind === "hotkey" || kind === "text" || kind === "url" || kind === "obs_scene") && (
        <Field
          label={
            kind === "hotkey"
              ? "Keys"
              : kind === "text"
                ? "Text"
                : kind === "url"
                  ? "Web address"
                  : "Scene name"
          }
          value={button.action.value}
          maxLength={kind === "text" ? 2000 : 300}
          placeholder={
            kind === "hotkey"
              ? "ctrl+shift+t"
              : kind === "url"
                ? "https://…"
                : kind === "obs_scene"
                  ? "Starting soon"
                  : ""
          }
          hint={
            kind === "hotkey"
              ? "Modifiers ctrl, alt, shift, meta + a key: a–z, 0–9, f1–f12, tab, enter, esc, space, arrows, home, end…"
              : kind === "obs_scene"
                ? `OBS WebSocket at ${settings.obs_url} (OBS → Tools → WebSocket Server Settings).`
                : undefined
          }
          onChange={(v) => setAction({ value: v })}
        />
      )}
      {kind === "obs_scene" && <ObsSettings settings={settings} />}
      <div className={p.row}>
        <Button
          icon="play"
          onPress={async () => {
            try {
              await bridge.tryDeckAction(button.action);
              toast(`“${button.label}” worked.`, "ok");
            } catch (e) {
              toast(err(e), "danger");
            }
          }}
          isDisabled={!button.action.value}
        >
          Try it here
        </Button>
        <Button variant="ghost" icon="trash" onPress={onDelete}>
          Delete
        </Button>
      </div>
    </div>
  );
}

function ObsSettings({ settings }: { settings: RemoteSettings }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [url, setUrl] = useState(settings.obs_url);
  const [password, setPassword] = useState("");
  return (
    <div className={s.obs}>
      <Field label="OBS WebSocket address" value={url} onChange={setUrl} maxLength={120} />
      <Field
        label="OBS WebSocket password"
        value={password}
        onChange={setPassword}
        maxLength={120}
        placeholder={
          settings.obs_password_set ? "Saved — type to change" : "Leave empty if authentication is off"
        }
      />
      <Button
        size="sm"
        onPress={() =>
          bridge.setObs(url, password ? password : null).then(
            () => toast("OBS settings saved.", "ok"),
            (e) => toast(err(e), "danger"),
          )
        }
      >
        Save OBS settings
      </Button>
    </div>
  );
}

// --------------------------------------------------------------- commands

function CommandsEditor({ settings, onSaved }: { settings: RemoteSettings; onSaved: () => void }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [list, setList] = useState<SavedCommand[]>(settings.commands);
  const [outputs, setOutputs] = useState<Record<string, { ok: boolean; text: string }>>({});
  const dirty = JSON.stringify(list) !== JSON.stringify(settings.commands);

  const update = (id: string, patch: Partial<SavedCommand>) =>
    setList(list.map((c) => (c.id === id ? { ...c, ...patch } : c)));

  return (
    <section className={p.section} aria-labelledby="cmd-h">
      <h2 id="cmd-h" className="visually-hidden">
        Saved commands
      </h2>
      <p className={p.sectionIntro}>
        Scripts your phone, a deck button or <code className={s.code}>devti run</code> can start on this
        computer. Devices see only the name; the command line stays here. Each run is limited to 60 seconds
        and logged in Security → Activity.
      </p>
      <div className={s.commands}>
        {list.map((c) => (
          <div className={s.command} key={c.id}>
            <div className={s.commandFields}>
              <Field label="Name" value={c.name} maxLength={80} onChange={(v) => update(c.id, { name: v })} />
              <Field
                label="Command"
                value={c.command}
                maxLength={2000}
                placeholder={
                  navigator.userAgent.includes("Windows") ? "start notepad" : "notify-send 'Hello'"
                }
                onChange={(v) => update(c.id, { command: v })}
              />
            </div>
            <div className={s.commandActions}>
              <Switch isSelected={c.confirm} onChange={(v) => update(c.id, { confirm: v })}>
                Ask “are you sure?” first
              </Switch>
              <Button
                size="sm"
                icon="play"
                isDisabled={!c.command.trim()}
                onPress={async () => {
                  const [ok, text] = await bridge.testCommand(c.command);
                  setOutputs({
                    ...outputs,
                    [c.id]: { ok, text: text || (ok ? "Done, no output." : "Failed.") },
                  });
                }}
              >
                Test
              </Button>
              <Button
                size="sm"
                variant="ghost"
                icon="trash"
                onPress={() => setList(list.filter((x) => x.id !== c.id))}
              >
                Delete
              </Button>
            </div>
            {outputs[c.id] && (
              <pre className={s.output} data-ok={outputs[c.id]?.ok || undefined}>
                {outputs[c.id]?.text}
              </pre>
            )}
          </div>
        ))}
        {!list.length && <p className={p.itemHint}>No saved commands yet.</p>}
      </div>
      <div className={p.row}>
        <Button
          icon="plus"
          onPress={() => setList([...list, { id: newId(), name: "", command: "", confirm: false }])}
        >
          Add a command
        </Button>
        <Button
          variant="primary"
          isDisabled={!dirty}
          onPress={() =>
            bridge.saveCommands(list).then(
              () => {
                toast("Saved commands updated.", "ok");
                onSaved();
              },
              (e) => toast(err(e), "danger"),
            )
          }
        >
          Save changes
        </Button>
      </div>
    </section>
  );
}

// -------------------------------------------------------------------- API

function ApiPanel({ settings, onChange }: { settings: RemoteSettings; onChange: () => void }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [api, setApi] = useState(settings.api);
  const [reveal, setReveal] = useState(false);
  const port = api.port ?? 47811;
  return (
    <section className={p.section} aria-labelledby="api-h">
      <h2 id="api-h" className="visually-hidden">
        Local API
      </h2>
      <div className={p.panel}>
        <div className={p.item}>
          <div className={p.itemText}>
            <span className={p.itemTitle}>Local API</span>
            <span className={p.itemHint}>
              Lets scripts on this computer use Devti. It only listens on 127.0.0.1 and needs the token below.
            </span>
          </div>
          <Switch
            isSelected={api.enabled}
            onChange={(v) =>
              bridge.setLocalApi(v).then(
                (a) => {
                  setApi(a);
                  onChange();
                },
                (e) => toast(err(e), "danger"),
              )
            }
          >
            <span className="visually-hidden">Local API</span>
          </Switch>
        </div>
        <div className={p.item}>
          <div className={p.itemText}>
            <span className={p.itemTitle}>Token</span>
            <span className={`${p.itemHint} ${p.mono}`}>
              {reveal ? api.token : `${api.token.slice(0, 6)}${"•".repeat(26)}`}
            </span>
          </div>
          <div className={p.row}>
            <Button size="sm" variant="ghost" icon="eye" onPress={() => setReveal(!reveal)}>
              {reveal ? "Hide" : "Show"}
            </Button>
            <Button
              size="sm"
              icon="copy"
              onPress={() => void bridge.copyToClipboard(api.token).then(() => toast("Token copied.", "ok"))}
            >
              Copy
            </Button>
            <Button
              size="sm"
              variant="danger"
              onPress={() =>
                bridge.resetApiToken().then(
                  (a) => {
                    setApi(a);
                    toast("New token made. Scripts using the old one stop working.", "info");
                  },
                  (e) => toast(err(e), "danger"),
                )
              }
            >
              Reset
            </Button>
          </div>
        </div>
      </div>
      <h3 className={s.subhead}>The devti command</h3>
      <p className={p.sectionIntro}>
        Installed with Devti. It finds the token by itself, so it just works in your terminal.
      </p>
      <pre className={s.codeBlock}>
        {`devti devices                      # who's online
devti send "Pixel 8" photo.jpg     # send files
devti ring "Pixel 8"               # find my phone
devti clip "hello"                 # share text to every clipboard
devti run backup                   # a saved command, here
devti run torch --on "Pixel 8"     # …or on another device
devti watch                        # every event, as JSON lines`}
      </pre>
      <h3 className={s.subhead}>HTTP</h3>
      <pre className={s.codeBlock}>
        {`curl -H "Authorization: Bearer $(cat ~/.local/share/app.devti.hub/api-token)" \\
     http://127.0.0.1:${port}/v1/devices`}
      </pre>
    </section>
  );
}
