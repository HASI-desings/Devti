import type { CSSProperties, DragEvent } from "react";
import { useCallback, useEffect, useState } from "react";
import type { Edge, ScreenLayout, ScreenSources, SourceInfo } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { orderedPeers, useStore } from "../../state/store";
import { Button, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Screens.module.css";

const EDGES: { edge: Edge; label: string }[] = [
  { edge: "left", label: "Left" },
  { edge: "top", label: "Above" },
  { edge: "right", label: "Right" },
  { edge: "bottom", label: "Below" },
];
const err = (e: unknown) => String(e instanceof Error ? e.message : e);
const same = (a: SourceInfo["source"] | null, b: SourceInfo["source"] | null) =>
  !!a && !!b && a.kind === b.kind && a.id === b.id;

/** Hub → Screens: what this computer shows to other devices, and where computers sit around it. */
export function ScreensPage() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const peers = useStore((st) => st.peers);
  const voices = useStore((st) => st.voices);
  const viewers = useStore((st) => st.viewers);
  const stats = useStore((st) => st.screenStats);
  const [src, setSrc] = useState<ScreenSources | null>(null);
  const [layout, setLayout] = useState<ScreenLayout>({ enabled: false, links: [] });
  const [supported, setSupported] = useState(true);
  const [dragOver, setDragOver] = useState<Edge | null>(null);

  const reload = useCallback(async () => {
    try {
      setSrc(await bridge.screenSources());
      const l = await bridge.screenLayout();
      setLayout(l.layout);
      setSupported(l.supported);
    } catch (e) {
      toast(err(e), "danger");
    }
  }, [bridge, toast]);
  useEffect(() => {
    void reload();
  }, [reload]);

  const computers = orderedPeers(peers).filter((x) => x.info.kind === "desktop" || x.info.kind === "laptop");
  const monitors = src?.sources.filter((x) => x.source.kind === "monitor") ?? [];
  const windows = src?.sources.filter((x) => x.source.kind === "window") ?? [];
  const selected = src?.selected ?? null;

  const saveLayout = (next: ScreenLayout) => {
    setLayout(next);
    bridge.setScreenLayout(next).catch((e) => toast(err(e), "danger"));
  };
  const place = (device: string | null, edge: Edge) => {
    const links = layout.links.filter((l) => l.edge !== edge && l.device !== device);
    saveLayout({ ...layout, links: device ? [...links, { device, edge }] : links });
  };
  const choose = (source: SourceInfo["source"] | null) => {
    bridge.setScreenSource(source).then(
      () => setSrc(src ? { ...src, selected: source } : src),
      (e) => toast(err(e), "danger"),
    );
  };

  // Monitors drawn at their real arrangement, scaled into the stage.
  const minX = Math.min(...monitors.map((m) => m.x), 0);
  const minY = Math.min(...monitors.map((m) => m.y), 0);
  const maxX = Math.max(...monitors.map((m) => m.x + m.width), 1);
  const maxY = Math.max(...monitors.map((m) => m.y + m.height), 1);
  const scale = Math.min(420 / (maxX - minX), 200 / (maxY - minY));

  const zone = (edge: Edge, label: string) => {
    const link = layout.links.find((l) => l.edge === edge);
    const peer = link ? peers[link.device] : undefined;
    return (
      // biome-ignore lint/a11y/noStaticElementInteractions: a drop target; the select inside is the keyboard way to place a computer
      <div
        key={edge}
        className={s.zone}
        data-edge={edge}
        data-over={dragOver === edge || undefined}
        data-filled={!!peer || undefined}
        style={peer ? ({ "--voice": voiceVar(voices[peer.info.id] ?? "iris") } as CSSProperties) : undefined}
        onDragOver={(e: DragEvent) => {
          e.preventDefault();
          setDragOver(edge);
        }}
        onDragLeave={() => setDragOver(null)}
        onDrop={(e: DragEvent) => {
          e.preventDefault();
          setDragOver(null);
          const id = e.dataTransfer.getData("text/devti-device");
          if (id) place(id, edge);
        }}
      >
        {peer ? (
          <>
            <Icon
              name={deviceIcon(peer.info.kind)}
              size={22}
              dotColor={peer.online ? voiceVar(voices[peer.info.id] ?? "iris") : undefined}
            />
            <span className={s.zoneName}>{peer.info.name}</span>
          </>
        ) : (
          <span className={s.zoneHint}>{label}</span>
        )}
        <select
          className={s.zoneSelect}
          aria-label={`Computer ${label.toLowerCase()} this one`}
          value={link?.device ?? ""}
          onChange={(e) => place(e.target.value || null, edge)}
        >
          <option value="">{peer ? "Remove" : "Choose…"}</option>
          {computers.map((c) => (
            <option key={c.info.id} value={c.info.id}>
              {c.info.name}
            </option>
          ))}
        </select>
      </div>
    );
  };

  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Screens</h1>
          <p className={p.intro}>
            Show this computer on a phone, tablet or another computer — the whole screen or one window — and
            use one mouse and keyboard across your computers.
          </p>
        </header>

        <section className={p.section} aria-labelledby="watching">
          <h2 id="watching" className={p.sectionTitle}>
            Watching now
          </h2>
          {Object.keys(viewers).length === 0 ? (
            <p className={p.sectionIntro}>
              Nobody. On a phone, open Devti → <b>Screen</b> to use it as a second screen for this computer.
              On another computer, open this one in Devti and choose <b>Mirror and control</b>.
            </p>
          ) : (
            <div className={p.panel}>
              {Object.entries(viewers).map(([id, name]) => (
                <div className={p.item} key={id}>
                  <div className={p.itemText}>
                    <span className={p.itemTitle}>{name}</span>
                    <span className={p.itemHint}>
                      {stats[id]?.sharing
                        ? `${stats[id]?.fps} fps · ${stats[id]?.latency_ms} ms · ${((stats[id]?.kbps ?? 0) / 1000).toFixed(1)} Mbit/s`
                        : "Starting…"}
                    </span>
                  </div>
                  <Button
                    size="sm"
                    variant="danger"
                    onPress={() => bridge.stopScreenViewer(id).catch((e) => toast(err(e), "danger"))}
                  >
                    Stop
                  </Button>
                </div>
              ))}
            </div>
          )}
        </section>

        <section className={p.section} aria-labelledby="share">
          <h2 id="share" className={p.sectionTitle}>
            What to show
          </h2>
          {src?.wayland ? (
            <p className={p.sectionIntro}>
              On Wayland, your desktop asks which screen or window to share the moment a device starts
              watching — that's the system's privacy rule, and Devti follows it.
            </p>
          ) : (
            <>
              <fieldset className={s.sources}>
                <legend className="visually-hidden">Screen or window to show</legend>
                {monitors.map((m) => (
                  <SourceCard
                    key={`m${m.source.id}`}
                    info={m}
                    selected={same(selected, m.source) || (!selected && m.primary)}
                    onPick={() => choose(m.source)}
                  />
                ))}
                {windows.slice(0, 12).map((w) => (
                  <SourceCard
                    key={`w${w.source.id}`}
                    info={w}
                    selected={same(selected, w.source)}
                    onPick={() => choose(w.source)}
                  />
                ))}
              </fieldset>
              <p className={p.itemHint}>
                Using a device as an <b>extended</b> display needs a virtual monitor: install the free, signed
                Virtual Display Driver on Windows, or a dummy plug / <code>evdi</code> on Linux. It then
                appears above as one more screen — pick it, and the device shows it.
              </p>
            </>
          )}
        </section>

        <section className={p.section} aria-labelledby="kvm">
          <h2 id="kvm" className={p.sectionTitle}>
            One keyboard and mouse
          </h2>
          <p className={p.sectionIntro}>
            Drag a computer to an edge. Then push the pointer past that edge to control it with this mouse and
            keyboard; come back the same way, or press Scroll Lock.
          </p>
          {!supported ? (
            <p className={p.note}>
              Not available on this desktop: Wayland doesn't let apps capture the pointer yet. It works on X11
              and Windows.
            </p>
          ) : (
            <>
              <div className={p.panel}>
                <div className={p.item}>
                  <div className={p.itemText}>
                    <span className={p.itemTitle}>Move the pointer between computers</span>
                    <span className={p.itemHint}>The other computer must allow this one to control it.</span>
                  </div>
                  <Switch isSelected={layout.enabled} onChange={(v) => saveLayout({ ...layout, enabled: v })}>
                    <span className="visually-hidden">Move the pointer between computers</span>
                  </Switch>
                </div>
              </div>
              <div className={s.arrange}>
                {EDGES.map((e) => zone(e.edge, e.label))}
                <div className={s.stage} role="img" aria-label="This computer's screens, as arranged">
                  {monitors.map((m) => (
                    <div
                      key={m.source.id}
                      className={s.monitor}
                      style={{
                        left: (m.x - minX) * scale,
                        top: (m.y - minY) * scale,
                        width: m.width * scale,
                        height: m.height * scale,
                      }}
                    >
                      <span>{m.name}</span>
                    </div>
                  ))}
                </div>
              </div>
              {computers.length === 0 && (
                <p className={p.itemHint}>Pair another computer to place it here.</p>
              )}
              <ul className={s.chips} aria-label="Your computers">
                {computers.map((c) => (
                  <li
                    key={c.info.id}
                    className={s.chip}
                    draggable
                    onDragStart={(e) => e.dataTransfer.setData("text/devti-device", c.info.id)}
                    style={{ "--voice": voiceVar(voices[c.info.id] ?? "iris") } as CSSProperties}
                  >
                    <Icon
                      name={deviceIcon(c.info.kind)}
                      size={18}
                      dotColor={c.online ? voiceVar(voices[c.info.id] ?? "iris") : undefined}
                    />
                    {c.info.name}
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>
      </div>
    </div>
  );
}

function SourceCard({ info, selected, onPick }: { info: SourceInfo; selected: boolean; onPick: () => void }) {
  const monitor = info.source.kind === "monitor";
  return (
    <button type="button" aria-pressed={selected} className={s.source} onClick={onPick}>
      <span
        className={s.sourceArt}
        data-window={!monitor || undefined}
        style={{ aspectRatio: `${info.width} / ${info.height}` }}
      >
        <Icon name={monitor ? "monitor" : "windows"} size={22} />
      </span>
      <span className={s.sourceName}>{info.name}</span>
      <span className={s.sourceDetail}>
        {monitor ? info.detail : info.detail || "Window"}
        {info.primary ? " · main" : ""}
      </span>
    </button>
  );
}
