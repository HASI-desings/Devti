import type { CSSProperties } from "react";
import { useState } from "react";
import type { Permissions, TrustRole } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { clockTime, kindLabel, latencyLabel, platformLabel, relativeTime } from "../../lib/format";
import { useStore } from "../../state/store";
import { BatteryMeter, Button, Segmented, Sheet, Switch } from "../../ui";
import p from "../shell/page.module.css";
import { DeviceActions } from "./DeviceActions";
import { DeviceControl } from "./DeviceControl";
import s from "./DevicePage.module.css";

export const PERMISSION_COPY: { key: keyof Permissions; title: string; hint: string }[] = [
  {
    key: "screen_view",
    title: "See this computer's screen",
    hint: "Mirror or extend this screen onto the device.",
  },
  {
    key: "control",
    title: "Control this computer",
    hint: "Use the device as a touchpad, keyboard or remote.",
  },
  { key: "files", title: "Send files", hint: "Files it sends are saved in your received-files folder." },
  {
    key: "browse",
    title: "Browse shared folders",
    hint: "See and download what you share in Files → Browse.",
  },
  { key: "clipboard", title: "Share the clipboard", hint: "Copy on one device, paste on the other." },
  {
    key: "notifications",
    title: "Exchange notifications",
    hint: "Show the device's notifications here and back.",
  },
  { key: "commands", title: "Run saved commands", hint: "Trigger scripts and power actions you set up." },
  {
    key: "bluetooth",
    title: "Use Bluetooth accessories",
    hint: "Play through and control accessories connected here.",
  },
];

const PATH_COPY: Record<"lan" | "vpn" | "internet", string> = {
  lan: "over your local network",
  vpn: "through your private network",
  internet: "over the internet",
};

export function DevicePage() {
  const bridge = useBridge();
  const id = useStore((st) => st.deviceId);
  const peer = useStore((st) => (id ? st.peers[id] : undefined));
  const voice = useStore((st) => (id ? st.voices[id] : undefined));
  const navigate = useStore((st) => st.navigate);
  const toast = useStore((st) => st.toast);
  const [confirmRemove, setConfirmRemove] = useState(false);

  if (!peer) {
    return (
      <div className={p.page}>
        <div className={p.inner}>
          <p className={p.intro}>This device is no longer paired.</p>
          <Button onPress={() => navigate("ecosystem")}>Back to the ecosystem</Button>
        </div>
      </div>
    );
  }

  const color = voiceVar(voice ?? "iris");
  const setAccess = async (role: TrustRole, perms: Permissions) => {
    try {
      await bridge.setAccess(peer.info.id, role, perms);
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  };
  const ROLE_DEFAULT: Record<TrustRole, Permissions> = {
    trusted: {
      screen_view: true,
      control: true,
      files: true,
      clipboard: true,
      notifications: true,
      commands: true,
      bluetooth: true,
      browse: true,
    },
    restricted: {
      screen_view: false,
      control: false,
      files: true,
      clipboard: true,
      notifications: true,
      commands: false,
      bluetooth: false,
      browse: false,
    },
  };

  return (
    <div className={p.page} style={{ "--voice": color } as CSSProperties}>
      <div className={p.inner}>
        <Button
          variant="ghost"
          size="sm"
          icon="chevronLeft"
          className={p.back}
          onPress={() => navigate("ecosystem")}
        >
          Ecosystem
        </Button>
        <header className={s.hero}>
          <span className={s.badge} data-online={peer.online || undefined}>
            <Icon name={deviceIcon(peer.info.kind)} size={44} dotColor={peer.online ? color : undefined} />
          </span>
          <div className={p.hero}>
            <h1 className={p.title}>{peer.info.name}</h1>
            <p className={p.intro}>
              {kindLabel(peer.info.kind)} with {peer.info.os_version || platformLabel(peer.info.platform)}
              {peer.info.model ? `, ${peer.info.model}` : ""}.{" "}
              {peer.online ? "Connected now." : `Last seen ${relativeTime(peer.last_seen)}.`}
            </p>
          </div>
        </header>

        <div className={p.row}>
          <DeviceActions peer={peer} />
        </div>

        <section className={p.section} aria-labelledby="conn">
          <h2 id="conn" className={p.sectionTitle}>
            Connection
          </h2>
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>
                  {peer.online ? `Connected ${PATH_COPY[peer.connection?.kind ?? "lan"]}` : "Offline"}
                </span>
                <span className={p.itemHint}>
                  {peer.online && peer.connection
                    ? `Since ${clockTime(peer.connection.since)}, at ${peer.connection.address}`
                    : "Devti reconnects automatically on the same network — or from anywhere, with Settings → Reach from anywhere."}
                </span>
              </div>
              {peer.connection && (
                <span className={s.latency}>
                  {latencyLabel(peer.connection.rtt_ms)}{" "}
                  <span className="num">{peer.connection.rtt_ms} ms</span>
                </span>
              )}
            </div>
            {peer.status.battery && (
              <div className={p.item}>
                <div className={p.itemText}>
                  <span className={p.itemTitle}>Battery</span>
                  <span className={p.itemHint}>
                    {peer.online ? "Live from the device." : "Last reported level."}
                  </span>
                </div>
                <BatteryMeter battery={peer.status.battery} accent={color} />
              </div>
            )}
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Paired</span>
                <span className={p.itemHint}>{clockTime(peer.paired_at)}</span>
              </div>
            </div>
          </div>
        </section>

        <DeviceControl peer={peer} />

        <section className={p.section} aria-labelledby="access">
          <h2 id="access" className={p.sectionTitle}>
            What {peer.info.name} may do here
          </h2>
          <p className={p.sectionIntro}>
            These choices are enforced by this computer. The other device can't change them.
          </p>
          <Segmented<TrustRole>
            label="Access level"
            value={peer.role}
            onChange={(role) => void setAccess(role, ROLE_DEFAULT[role])}
            options={[
              { value: "trusted", label: "Trusted", icon: "shield" },
              { value: "restricted", label: "Restricted", icon: "lock" },
            ]}
          />
          <div className={p.panel}>
            {PERMISSION_COPY.map((perm) => (
              <div className={p.item} key={perm.key}>
                <div className={p.itemText}>
                  <span className={p.itemTitle}>{perm.title}</span>
                  <span className={p.itemHint}>{perm.hint}</span>
                </div>
                <Switch
                  isSelected={peer.permissions[perm.key]}
                  onChange={(v) => void setAccess(peer.role, { ...peer.permissions, [perm.key]: v })}
                  accent={color}
                >
                  <span className="visually-hidden">{perm.title}</span>
                </Switch>
              </div>
            ))}
          </div>
        </section>

        <section className={p.section} aria-labelledby="identity">
          <h2 id="identity" className={p.sectionTitle}>
            Identity
          </h2>
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Security fingerprint</span>
                <span className={`${p.itemHint} ${p.mono}`}>{peer.fingerprint}</span>
              </div>
            </div>
          </div>
        </section>

        <section className={p.section} aria-labelledby="remove">
          <h2 id="remove" className={p.sectionTitle}>
            Remove
          </h2>
          <div className={p.panel}>
            <div className={p.item}>
              <div className={p.itemText}>
                <span className={p.itemTitle}>Remove {peer.info.name} from this ecosystem</span>
                <span className={p.itemHint}>
                  It disconnects at once and has to be paired again to come back.
                </span>
              </div>
              <Button variant="danger" icon="trash" onPress={() => setConfirmRemove(true)}>
                Remove device
              </Button>
            </div>
          </div>
        </section>
      </div>

      <Sheet isOpen={confirmRemove} onClose={() => setConfirmRemove(false)} label="Remove device" narrow>
        <div className={s.confirm}>
          <h2 className={p.sectionTitle}>Remove {peer.info.name}?</h2>
          <p className={p.sectionIntro}>
            {peer.info.name} will be disconnected and forgotten on both sides. You can pair it again later.
          </p>
          <div className={p.row}>
            <Button
              variant="danger"
              onPress={async () => {
                setConfirmRemove(false);
                try {
                  await bridge.revoke(peer.info.id);
                  toast(`Removed ${peer.info.name}.`, "ok");
                  navigate("ecosystem");
                } catch (e) {
                  toast(String(e instanceof Error ? e.message : e), "danger");
                }
              }}
            >
              Remove device
            </Button>
            <Button variant="quiet" onPress={() => setConfirmRemove(false)}>
              Keep it
            </Button>
          </div>
        </div>
      </Sheet>
    </div>
  );
}
