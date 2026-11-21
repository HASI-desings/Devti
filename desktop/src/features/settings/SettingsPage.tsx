import { useEffect, useState } from "react";
import type { SystemInfo } from "../../bridge";
import { Logo } from "../../design/Prism";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button, Field, Segmented, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./SettingsPage.module.css";

const CREDITS: [string, string][] = [
  ["Tauri", "MIT / Apache-2.0"],
  ["quinn (QUIC)", "MIT / Apache-2.0"],
  ["rustls + ring", "Apache-2.0 / ISC"],
  ["SPAKE2 (RustCrypto)", "MIT / Apache-2.0"],
  ["mdns-sd", "MIT / Apache-2.0"],
  ["SQLite + rusqlite", "Public domain / MIT"],
  ["React", "MIT"],
  ["Motion", "MIT"],
  ["React Aria", "Apache-2.0"],
  ["Bricolage Grotesque", "SIL Open Font License"],
  ["Instrument Sans", "SIL Open Font License"],
  ["uqr", "MIT"],
];

export function SettingsPage() {
  const bridge = useBridge();
  const settings = useStore((st) => st.settings);
  const setSetting = useStore((st) => st.setSetting);
  const local = useStore((st) => st.local)!;
  const toast = useStore((st) => st.toast);
  const [name, setName] = useState(local.info.name);
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const downloadsDir = useStore((st) => st.downloadsDir);
  const threshold = useStore((st) => st.lowBatteryThreshold);

  useEffect(() => {
    void bridge.systemInfo().then(setSys);
  }, [bridge]);

  const save = (key: string, value: string) => {
    setSetting(key, value);
    void bridge.setSetting(key, value);
  };

  const rename = async () => {
    try {
      const l = await bridge.rename(name);
      setName(l.info.name);
      toast(`This computer is now called ${l.info.name}.`, "ok");
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  };

  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Settings</h1>
        </header>

        <section className={p.section} aria-labelledby="look">
          <h2 id="look" className={p.sectionTitle}>
            Appearance
          </h2>
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Theme</span>
                <span className={p.itemHint}>Night and Day share every color; only the light changes.</span>
              </div>
              <Segmented
                label="Theme"
                value={settings.theme ?? "system"}
                onChange={(v) => save("theme", v)}
                options={[
                  { value: "system", label: "System" },
                  { value: "dark", label: "Night", icon: "moon" },
                  { value: "light", label: "Day", icon: "sun" },
                ]}
              />
            </div>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Motion</span>
                <span className={p.itemHint}>
                  Reduced motion keeps every state change but drops drift and parallax.
                </span>
              </div>
              <Segmented
                label="Motion"
                value={settings.motion ?? "system"}
                onChange={(v) => save("motion", v)}
                options={[
                  { value: "system", label: "System" },
                  { value: "full", label: "Full" },
                  { value: "reduced", label: "Reduced" },
                ]}
              />
            </div>
          </div>
        </section>

        <section className={p.section} aria-labelledby="startup">
          <h2 id="startup" className={p.sectionTitle}>
            Startup and tray
          </h2>
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Start Devti when you sign in</span>
                <span className={p.itemHint}>Your devices can reconnect without you opening the app.</span>
              </div>
              <Switch
                isSelected={sys?.autostart ?? settings.autostart === "true"}
                onChange={async (v) => {
                  try {
                    const on = await bridge.setAutostart(v);
                    setSys((x) => (x ? { ...x, autostart: on } : x));
                    setSetting("autostart", String(on));
                  } catch (e) {
                    toast(String(e instanceof Error ? e.message : e), "danger");
                  }
                }}
              >
                <span className="visually-hidden">Start Devti when you sign in</span>
              </Switch>
            </div>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Keep running in the tray when the window closes</span>
                <span className={p.itemHint}>
                  Closing the window frees its memory; devices stay connected. Quit from the tray icon.
                </span>
              </div>
              <Switch
                isSelected={settings.close_to_tray !== "false"}
                onChange={(v) => save("close_to_tray", String(v))}
              >
                <span className="visually-hidden">Keep running in the tray</span>
              </Switch>
            </div>
          </div>
        </section>

        <section className={p.section} aria-labelledby="continuity">
          <h2 id="continuity" className={p.sectionTitle}>
            Files and battery
          </h2>
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Save received files in</span>
                <span className={`${p.itemHint} ${p.mono}`}>{downloadsDir || "Downloads/Devti"}</span>
              </div>
              <Button
                onPress={async () => {
                  try {
                    const dir = await bridge.chooseDownloadsDir();
                    if (dir) useStore.setState({ downloadsDir: dir });
                  } catch (e) {
                    toast(String(e instanceof Error ? e.message : e), "danger");
                  }
                }}
              >
                Change
              </Button>
            </div>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Warn me when a device's battery drops to</span>
                <span className={p.itemHint}>Once per discharge, as a notification on this computer.</span>
              </div>
              <Segmented
                label="Low battery alert"
                value={String(threshold)}
                onChange={(v) => {
                  useStore.setState({ lowBatteryThreshold: Number(v) });
                  void bridge.setLowBatteryThreshold(Number(v));
                }}
                options={[
                  { value: "10", label: "10%" },
                  { value: "15", label: "15%" },
                  { value: "20", label: "20%" },
                  { value: "30", label: "30%" },
                ]}
              />
            </div>
          </div>
        </section>

        <section className={p.section} aria-labelledby="device">
          <h2 id="device" className={p.sectionTitle}>
            This computer
          </h2>
          <div className={p.panel}>
            <div className={`${p.item} ${s.renameRow}`}>
              <Field
                label="Name shown on your other devices"
                value={name}
                onChange={setName}
                maxLength={48}
                onSubmit={() => void rename()}
              />
              <Button
                onPress={() => void rename()}
                isDisabled={name.trim() === local.info.name || !name.trim()}
              >
                Rename
              </Button>
            </div>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Network address</span>
                <span className={`${p.itemHint} ${p.mono}`}>
                  {local.addresses.length
                    ? local.addresses.map((a) => `${a}:${local.port}`).join(", ")
                    : "Not connected to a network"}
                </span>
              </div>
            </div>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Find devices automatically</span>
                <span className={p.itemHint}>
                  {local.discovery_enabled
                    ? "Devti announces itself on your local network so your devices find it. QR pairing works even if your router blocks this."
                    : "Automatic discovery is off. Pair with the QR code or an address."}
                </span>
              </div>
            </div>
          </div>
        </section>

        <section className={p.section} aria-labelledby="diag">
          <h2 id="diag" className={p.sectionTitle}>
            Diagnostics
          </h2>
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Export diagnostics</span>
                <span className={p.itemHint}>
                  Saves recent logs with keys, codes and personal paths removed, for a bug report.
                </span>
              </div>
              <Button
                icon="download"
                onPress={async () => {
                  try {
                    toast(await bridge.exportDiagnostics(), "ok");
                  } catch (e) {
                    toast(String(e instanceof Error ? e.message : e), "danger");
                  }
                }}
              >
                Export
              </Button>
            </div>
            {sys && (
              <div className={p.item}>
                <div className={p.itemText}>
                  <span className={p.itemTitle}>Data folder</span>
                  <span className={`${p.itemHint} ${p.mono}`}>{sys.data_dir}</span>
                </div>
              </div>
            )}
          </div>
        </section>

        <section className={p.section} aria-labelledby="about">
          <h2 id="about" className={p.sectionTitle}>
            About
          </h2>
          <div className={s.about}>
            <Logo size={56} />
            <div className={p.itemText}>
              <span className={s.aboutName}>Devti {sys?.app_version ?? local.info.app_version}</span>
              <span className={p.itemHint}>
                Free and open source under the GNU General Public License v3.0 or later. No accounts, no
                cloud, no tracking.
              </span>
              <div className={p.row}>
                <Button
                  variant="ghost"
                  size="sm"
                  icon="external"
                  onPress={() => void bridge.openExternal("https://github.com/hasi-desings/devti")}
                >
                  Source code
                </Button>
              </div>
            </div>
          </div>
          <details className={s.credits}>
            <summary>Open-source software in Devti</summary>
            <ul>
              {CREDITS.map(([n, l]) => (
                <li key={n}>
                  <span>{n}</span>
                  <span className={p.itemHint}>{l}</span>
                </li>
              ))}
            </ul>
          </details>
        </section>
      </div>
    </div>
  );
}
