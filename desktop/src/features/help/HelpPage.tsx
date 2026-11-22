import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button } from "../../ui";
import p from "../shell/page.module.css";
import s from "./HelpPage.module.css";

type Support = "yes" | "no" | "partial";
const MATRIX: { feature: string; cells: [Support, Support, Support, Support, Support]; note?: string }[] = [
  { feature: "Pair with QR code or 6-digit code", cells: ["yes", "yes", "yes", "yes", "no"] },
  { feature: "Encrypted, automatic reconnection", cells: ["yes", "yes", "yes", "yes", "no"] },
  { feature: "Live battery level", cells: ["yes", "yes", "yes", "yes", "no"] },
  { feature: "Per-device permissions and pause", cells: ["yes", "yes", "yes", "yes", "no"] },
  {
    feature: "Activity log",
    cells: ["yes", "yes", "yes", "partial", "no"],
    note: "Phones keep their own log.",
  },
  {
    feature: "Clipboard sync",
    cells: ["yes", "yes", "yes", "partial", "no"],
    note: "Android only lets Devti read the clipboard while it's open; text from the computer arrives any time.",
  },
  { feature: "Files of any size, resumable", cells: ["yes", "yes", "yes", "yes", "no"] },
  {
    feature: "Phone notifications with actions and replies",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "iOS doesn't let apps read other apps' notifications.",
  },
  { feature: "Do Not Disturb everywhere, find my device", cells: ["yes", "yes", "yes", "yes", "no"] },
  {
    feature: "Controlled from a phone (touchpad, keys, media, power)",
    cells: ["yes", "yes", "partial", "no", "no"],
    note: "Wayland needs libei or virtual-input support in the desktop; GNOME asks once.",
  },
  { feature: "Shortcuts deck", cells: ["yes", "yes", "yes", "yes", "no"] },
  {
    feature: "Mirror and control an Android phone",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "On the phone: allow screen sharing each time, and turn on Devti control in Accessibility.",
  },
  { feature: "Local API and devti command", cells: ["yes", "yes", "yes", "no", "no"] },
  {
    feature: "Show this screen or a window on other devices",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "On Wayland the system asks which screen or window each time. Android shows it.",
  },
  {
    feature: "Bluetooth accessories: battery, signal, health",
    cells: ["partial", "yes", "yes", "no", "no"],
    note: "Windows shows BLE accessories and standard batteries.",
  },
  {
    feature: "Accessory controls (noise control, EQ…)",
    cells: ["no", "yes", "yes", "no", "no"],
    note: "Needs Linux (BlueZ) for the accessories' own channels for now.",
  },
  {
    feature: "One keyboard and mouse across computers",
    cells: ["yes", "yes", "no", "no", "no"],
    note: "Wayland doesn't let apps capture the pointer yet.",
  },
  {
    feature: "Texts from an Android phone: read, reply, new",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "The phone sends from its own number. iPhone doesn't let apps read texts.",
  },
  {
    feature: "Calls: who's calling, answer, decline, mute, end",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "Call audio stays on the phone; pair it as a Bluetooth headset to talk here.",
  },
  { feature: "Continue on another device", cells: ["yes", "yes", "yes", "yes", "no"] },
  {
    feature: "Phone sound and microphone on this computer",
    cells: ["partial", "yes", "yes", "yes", "no"],
    note: "Windows needs the free VB-Audio Virtual Cable for the microphone. Phone sound needs Android 10.",
  },
  {
    feature: "Phone camera as a webcam",
    cells: ["no", "yes", "yes", "yes", "no"],
    note: "Linux uses v4l2loopback. On Windows the camera shows in Devti.",
  },
  {
    feature: "Rules, and locking when your phone walks away",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "Walk-away needs Bluetooth on this computer.",
  },
  {
    feature: "Reach from anywhere",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "Through Tailscale, WireGuard, ZeroTier or a forwarded port.",
  },
  {
    feature: "Browse and download shared folders",
    cells: ["yes", "yes", "yes", "yes", "no"],
    note: "Read-only. Android can share its media and document folders.",
  },
  { feature: "WebAssembly plugins", cells: ["yes", "yes", "yes", "no", "no"], note: "Run on the computer." },
];
const COLUMNS = ["Windows", "Linux X11", "Linux Wayland", "Android", "iPhone and iPad"];

const TROUBLE: { q: string; a: string }[] = [
  {
    q: "My phone can't find this computer",
    a: "Make sure both are on the same Wi-Fi network, not a guest network. Some routers block device discovery; the QR code still works because it carries this computer's address. On Windows, allow Devti through the firewall when Windows asks, or in Windows Security → Firewall → Allow an app.",
  },
  {
    q: "The codes on both screens are different",
    a: "Tap “They're different” on both devices. Different codes mean the connection did not go straight between your two devices. Try again on a network you trust.",
  },
  {
    q: "Windows says “Windows protected your PC”",
    a: "Devti's installer is not signed with a paid certificate yet. Choose More info → Run anyway. You can check the download against the checksum on the releases page.",
  },
  {
    q: "Linux on Wayland asks for permission",
    a: "On Wayland, screen sharing and remote input always go through your desktop's permission dialog. That's the system protecting you; Devti asks only when a feature needs it.",
  },
  {
    q: "My Android phone disconnects when the screen is off",
    a: "Open Android Settings → Apps → Devti → Battery and choose Unrestricted. Some phone brands add their own battery savers; dontkillmyapp.com lists the steps for each one.",
  },
  {
    q: "Bluetooth accessories don't show up",
    a: "Check that Bluetooth is on and the accessory is in pairing mode. On Linux, the bluetooth service must be running (systemctl status bluetooth).",
  },
];

export function HelpPage({ onRerunSetup }: { onRerunSetup: () => void }) {
  const bridge = useBridge();
  const setTour = useStore((st) => st.setTour);
  const navigate = useStore((st) => st.navigate);
  return (
    <div className={p.page}>
      <div className={p.inner}>
        <header className={p.hero}>
          <h1 className={p.title}>Help</h1>
          <p className={p.intro}>
            How Devti works, what it can do on each system, and what to try when something is off.
          </p>
          <div className={p.row}>
            <Button
              icon="sparkle"
              onPress={() => {
                navigate("ecosystem");
                setTour(true);
              }}
            >
              Take the tour
            </Button>
            <Button variant="ghost" onPress={onRerunSetup}>
              Run setup again
            </Button>
          </div>
        </header>

        <section className={p.section} aria-labelledby="how">
          <h2 id="how" className={p.sectionTitle}>
            How Devti works
          </h2>
          <div className={s.how}>
            <div>
              <Icon name="hub" size={26} />
              <h3>One Hub</h3>
              <p>
                This computer is the center. It holds the connections, the list of trusted devices and their
                permissions.
              </p>
            </div>
            <div>
              <Icon name="link" size={26} />
              <h3>Paired once</h3>
              <p>
                Each device proves its identity once with a QR code or a code, and you confirm a 6-digit
                check. After that they reconnect by themselves.
              </p>
            </div>
            <div>
              <Icon name="lock" size={26} />
              <h3>Only between your devices</h3>
              <p>
                Everything travels encrypted, directly over your network. No account, no server, nothing in
                the cloud.
              </p>
            </div>
          </div>
        </section>

        <section className={p.section} aria-labelledby="where">
          <h2 id="where" className={p.sectionTitle}>
            What works where
          </h2>
          <p className={p.sectionIntro}>
            This is the first Devti release, built on the foundation every later feature uses. The iPhone and
            iPad app comes in a later release.
          </p>
          <div className={`${p.panel} ${s.tableWrap}`}>
            <table className={s.table}>
              <thead>
                <tr>
                  <th scope="col">Feature</th>
                  {COLUMNS.map((c) => (
                    <th scope="col" key={c}>
                      {c}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {MATRIX.map((row) => (
                  <tr key={row.feature}>
                    <th scope="row">
                      {row.feature}
                      {row.note && <span className={s.note}>{row.note}</span>}
                    </th>
                    {row.cells.map((c, i) => (
                      <td key={COLUMNS[i]} data-support={c}>
                        <span className="visually-hidden">
                          {c === "yes" ? "Supported" : c === "partial" ? "Partly supported" : "Not yet"}
                        </span>
                        <Icon name={c === "yes" ? "check" : c === "partial" ? "info" : "close"} size={16} />
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <section className={p.section} aria-labelledby="trouble">
          <h2 id="trouble" className={p.sectionTitle}>
            Troubleshooting
          </h2>
          <div className={s.faq}>
            {TROUBLE.map((t) => (
              <details key={t.q}>
                <summary>
                  {t.q}
                  <Icon name="chevronDown" size={18} />
                </summary>
                <p>{t.a}</p>
              </details>
            ))}
          </div>
          <div className={p.row}>
            <Button
              variant="ghost"
              icon="external"
              onPress={() => void bridge.openExternal("https://github.com/hasi-desings/devti/issues")}
            >
              Report a problem
            </Button>
          </div>
        </section>
      </div>
    </div>
  );
}
