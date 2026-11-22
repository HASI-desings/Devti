import { useEffect, useState } from "react";
import type { PeerSnapshot, SystemAction } from "../../bridge";
import { Icon, type IconName } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { relativeTime } from "../../lib/format";
import { useStore } from "../../state/store";
import { Button, Sheet } from "../../ui";
import p from "../shell/page.module.css";
import s from "./DevicePage.module.css";

const POWER: { a: SystemAction["a"]; label: string; icon: IconName; confirm?: string }[] = [
  { a: "lock", label: "Lock", icon: "lock" },
  { a: "sleep", label: "Sleep", icon: "moon" },
  { a: "restart", label: "Restart", icon: "power", confirm: "Unsaved work on it may be lost." },
  { a: "shutdown", label: "Shut down", icon: "power", confirm: "Unsaved work on it may be lost." },
  { a: "sign_out", label: "Sign out", icon: "external", confirm: "Its open apps will close." },
];

/** What this computer can do *to* another device: mirror, media, power, its commands and apps. */
export function DeviceControl({ peer }: { peer: PeerSnapshot }) {
  const bridge = useBridge();
  const catalog = useStore((st) => st.catalogs[peer.info.id]);
  const allResults = useStore((st) => st.runResults);
  const results = allResults.filter((r) => r.from === peer.info.id).slice(0, 3);
  const navigate = useStore((st) => st.navigate);
  const toast = useStore((st) => st.toast);
  const [confirm, setConfirm] = useState<{ label: string; detail: string; run: () => void } | null>(null);
  const id = peer.info.id;
  const android = peer.info.platform === "android";

  useEffect(() => {
    if (peer.online) void bridge.requestCatalog(id).catch(() => {});
  }, [bridge, id, peer.online]);

  const act = (fn: () => Promise<unknown>) => {
    fn().catch((e) => toast(String(e instanceof Error ? e.message : e), "danger"));
  };
  const media = (action: string) => act(() => bridge.sendInput(id, [{ k: "media", action }]));
  const system = (action: SystemAction) => act(() => bridge.systemAction(id, action));

  return (
    <section className={p.section} aria-labelledby="control">
      <h2 id="control" className={p.sectionTitle}>
        Control
      </h2>
      {!peer.online ? (
        <p className={p.sectionIntro}>Available when {peer.info.name} is connected.</p>
      ) : (
        <>
          {android && (
            <div className={s.mirrorCard}>
              <Icon name="cast" size={28} />
              <div className={p.itemText}>
                <span className={p.itemTitle}>See and control {peer.info.name}'s screen</span>
                <span className={p.itemHint}>
                  Click to tap, drag to swipe, type with this keyboard. Take screenshots and recordings.
                </span>
              </div>
              <Button variant="primary" icon="cast" onPress={() => navigate("mirror", id)}>
                Mirror and control
              </Button>
            </div>
          )}

          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Media</span>
                <span className={p.itemHint}>Controls whatever is playing on {peer.info.name}.</span>
              </div>
              <div className={p.row}>
                <Button size="sm" variant="ghost" iconOnly icon="previous" onPress={() => media("previous")}>
                  Previous
                </Button>
                <Button size="sm" iconOnly icon="play" onPress={() => media("play_pause")}>
                  Play or pause
                </Button>
                <Button size="sm" variant="ghost" iconOnly icon="next" onPress={() => media("next")}>
                  Next
                </Button>
                <Button size="sm" variant="ghost" iconOnly icon="mute" onPress={() => media("volume_down")}>
                  Volume down
                </Button>
                <Button size="sm" variant="ghost" iconOnly icon="volume" onPress={() => media("volume_up")}>
                  Volume up
                </Button>
              </div>
            </div>
            {catalog?.system.some((a) => POWER.some((x) => x.a === a)) && (
              <div className={p.item}>
                <div className={p.itemText}>
                  <span className={p.itemTitle}>Power</span>
                  <span className={p.itemHint}>Needs “Run saved commands” allowed on {peer.info.name}.</span>
                </div>
                <div className={p.row}>
                  {POWER.filter((x) => catalog.system.includes(x.a)).map((x) => (
                    <Button
                      key={x.a}
                      size="sm"
                      variant="ghost"
                      icon={x.icon}
                      onPress={() =>
                        x.confirm
                          ? setConfirm({
                              label: `${x.label} ${peer.info.name}?`,
                              detail: x.confirm,
                              run: () => system({ a: x.a } as SystemAction),
                            })
                          : system({ a: x.a } as SystemAction)
                      }
                    >
                      {x.label}
                    </Button>
                  ))}
                </div>
              </div>
            )}
            {catalog?.commands.map((c) => (
              <div className={p.item} key={c.id}>
                <div className={p.itemText}>
                  <span className={p.itemTitle}>{c.name}</span>
                  <span className={p.itemHint}>Saved command on {peer.info.name}</span>
                </div>
                <Button
                  size="sm"
                  icon="play"
                  onPress={() =>
                    c.confirm
                      ? setConfirm({
                          label: `Run “${c.name}”?`,
                          detail: `It runs on ${peer.info.name}.`,
                          run: () => act(() => bridge.runRemoteCommand(id, c.id)),
                        })
                      : act(() => bridge.runRemoteCommand(id, c.id))
                  }
                >
                  Run
                </Button>
              </div>
            ))}
          </div>

          {results.length > 0 && (
            <div className={s.results} aria-live="polite">
              {results.map((r) => (
                <div key={r.id} className={s.result} data-ok={r.ok || undefined}>
                  <span className={p.itemHint}>
                    {catalog?.commands.find((c) => c.id === r.command)?.name ?? r.command} ·{" "}
                    {relativeTime(r.ts)}
                  </span>
                  <pre>{r.output || (r.ok ? "Done." : "Failed.")}</pre>
                </div>
              ))}
            </div>
          )}

          {catalog && catalog.apps.length > 0 && (
            <>
              <h3 className={s.subhead}>Open an app on {peer.info.name}</h3>
              <div className={s.apps}>
                {catalog.apps.slice(0, 60).map((a) => (
                  <button
                    type="button"
                    key={a.id}
                    className={s.app}
                    onClick={() => act(() => bridge.launchApp(id, a.id))}
                  >
                    <span className={s.appGlyph} aria-hidden="true">
                      {a.name.slice(0, 1)}
                    </span>
                    <span>{a.name}</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </>
      )}

      <Sheet isOpen={!!confirm} onClose={() => setConfirm(null)} label="Confirm" narrow>
        <div className={s.confirm}>
          <h2 className={p.sectionTitle}>{confirm?.label}</h2>
          <p className={p.sectionIntro}>{confirm?.detail}</p>
          <div className={p.row}>
            <Button
              variant="danger"
              onPress={() => {
                confirm?.run();
                setConfirm(null);
              }}
            >
              Yes, do it
            </Button>
            <Button variant="quiet" onPress={() => setConfirm(null)}>
              Cancel
            </Button>
          </div>
        </div>
      </Sheet>
    </section>
  );
}
