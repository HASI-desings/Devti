import { useEffect, useState } from "react";
import type { AnywhereInfo, ReachAddress, ReachInfo } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button, Field, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./SettingsPage.module.css";

const KIND: Record<ReachAddress["kind"], string> = {
  lan: "Local network",
  vpn: "Private network",
  internet: "Internet",
  manual: "Set up by you",
};

function anywhereStatus(a: AnywhereInfo): string {
  if (!a.enabled) return "Off. Your devices connect only on the same network (or through the ways below).";
  if (a.error) return `Couldn't start: ${a.error}`;
  if (!a.running) return "Starting…";
  if (!a.online) return "Looking for the internet… Devices on this network still connect.";
  return `On. Reachable from anywhere${a.relay ? ` through ${a.relay}` : ""}${
    a.public_ip ? `, from public address ${a.public_ip}` : ""
  }.`;
}

/** Settings → Connect from anywhere (built in since 1.1, D-54). */
function AnywherePanel() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [info, setInfo] = useState<AnywhereInfo | null>(null);
  const [relay, setRelay] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const load = () =>
      void bridge.anywhereInfo().then((i) => {
        setInfo(i);
        setRelay((r) => r || i.custom_relay);
      });
    load();
    // The relay connection comes up a few seconds after start; poll lightly until it does.
    const t = setInterval(load, 4000);
    const off = bridge.onEvent((e) => {
      if (e.type === "anywhere_changed") load();
    });
    return () => {
      clearInterval(t);
      off();
    };
  }, [bridge]);

  if (!info) return null;
  const save = async (enabled: boolean, r = relay) => {
    try {
      const next = await bridge.setAnywhere(enabled, r);
      setInfo(next);
      setError(null);
      toast(
        enabled
          ? "Your devices can now find each other from anywhere."
          : "Connecting from anywhere is off. Devices still connect on the same network.",
        "ok",
      );
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  return (
    <div className={p.panel}>
      <div className={p.item}>
        <div className={p.itemText}>
          <span className={p.itemTitle}>Connect from anywhere</span>
          <span className={p.itemHint} data-testid="anywhere-status">
            {anywhereStatus(info)}
          </span>
        </div>
        <Switch isSelected={info.enabled} onChange={(v) => void save(v)}>
          <span className="visually-hidden">Connect from anywhere</span>
        </Switch>
      </div>
      <div className={p.item}>
        <div className={s.reachField}>
          <Field
            label="Your own relay (optional)"
            value={relay}
            onChange={setRelay}
            placeholder="https://relay.example.org"
            hint="Leave empty to use the free public relays. Run iroh-relay on a server you control to use your own."
            error={error}
            onSubmit={() => void save(info.enabled)}
          />
        </div>
        <Button variant="quiet" size="sm" onPress={() => void save(info.enabled)}>
          Save
        </Button>
      </div>
    </div>
  );
}

/** Settings → Connect from anywhere, then the other ways (Phase 7). */
export function ReachSection() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [info, setInfo] = useState<ReachInfo | null>(null);
  const [extra, setExtra] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void bridge.reachInfo().then((i) => {
      setInfo(i);
      setExtra(i.extra.join(", "));
    });
  }, [bridge]);

  if (!info) return null;
  const entries = extra
    .split(/[\s,]+/)
    .map((e) => e.trim())
    .filter(Boolean);

  const save = async (enabled: boolean) => {
    try {
      const next = await bridge.setReach(enabled, entries);
      setInfo(next);
      setError(null);
      toast(
        enabled ? "Your devices can now reach this computer from anywhere." : "Remote access is off.",
        "ok",
      );
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  const remote = info.addresses.filter((a) => a.kind !== "lan");

  return (
    <section className={p.section} aria-labelledby="reach">
      <h2 id="reach" className={p.sectionTitle}>
        Connect from anywhere
      </h2>
      <p className={p.sectionIntro}>
        At home, at work or on mobile data, your devices find each other by their keys. They connect directly
        through both routers when they can, and through an encrypted relay when they can't. A relay passes
        along packets it can't read, and only devices you paired get in.
      </p>
      <AnywherePanel />
      <h3 className={s.subTitle}>Other ways to connect</h3>
      <div className={p.panel}>
        <div className={p.item}>
          <div className={p.itemText}>
            <span className={p.itemTitle}>Also share my VPN and typed addresses</span>
            <span className={p.itemHint}>
              Tells your paired devices the addresses below (Tailscale, WireGuard, a forwarded port). Useful
              when you'd rather not use a relay.
            </span>
          </div>
          <Switch isSelected={info.enabled} onChange={(v) => void save(v)}>
            <span className="visually-hidden">Reach from anywhere</span>
          </Switch>
        </div>
        <div className={p.item}>
          <div className={p.itemText}>
            <span className={p.itemTitle}>This computer can be reached at</span>
            <ul className={s.addresses}>
              {info.addresses.map((a) => (
                <li key={a.address} className={s.address} data-kind={a.kind}>
                  <span className={p.mono}>{a.address}</span>
                  <span className={s.addressKind}>
                    {KIND[a.kind]}
                    {a.interface ? ` · ${a.interface}` : ""}
                  </span>
                </li>
              ))}
            </ul>
            {info.enabled && remote.length === 0 && (
              <span className={p.itemHint}>
                Only local addresses so far. Install Tailscale on this computer and your phone, or add an
                address below.
              </span>
            )}
          </div>
        </div>
        <div className={p.item}>
          <div className={s.reachField}>
            <Field
              label="Addresses you set up yourself"
              value={extra}
              onChange={setExtra}
              placeholder={`home.example.org:${info.port}`}
              hint={`A forwarded port or a dynamic-DNS name, as host:port. Forward UDP port ${info.port} to this computer.`}
              error={error}
              onSubmit={() => void save(info.enabled)}
            />
          </div>
          <Button variant="quiet" size="sm" onPress={() => void save(info.enabled)}>
            Save
          </Button>
        </div>
      </div>
      <div className={s.ways}>
        <div className={s.way}>
          <Icon name="shield" size={20} />
          <strong>Tailscale or Headscale</strong>
          <span>
            Free and easiest: install it on each device and sign in once. Devti sees its addresses and uses
            them by itself. Headscale is the self-hosted version.
          </span>
        </div>
        <div className={s.way}>
          <Icon name="network" size={20} />
          <strong>WireGuard or ZeroTier</strong>
          <span>Your own private network. Devti finds its addresses on this computer the same way.</span>
        </div>
        <div className={s.way}>
          <Icon name="globe" size={20} />
          <strong>Port forwarding</strong>
          <span>
            Forward UDP {info.port} on your router to this computer and add your address above. Only devices
            you paired can connect.
          </span>
        </div>
      </div>
    </section>
  );
}
