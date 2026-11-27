import { useEffect, useState } from "react";
import type { GuardEntry, GuardSetupCheck, GuardState, PeerSnapshot } from "../../bridge";
import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import { relativeTime } from "../../lib/format";
import { useStore } from "../../state/store";
import { Button, Field, Sheet, Switch } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Stolen.module.css";

const err = (e: unknown) => String(e instanceof Error ? e.message : e);
const DEFAULT_MESSAGE = "This device is stolen. Return it to its owner. Police is on the way.";

const LEVEL: Record<string, { label: string; tone: "ok" | "warn" | "danger" }> = {
  device_owner: { label: "Full protection", tone: "ok" },
  admin: { label: "Basic protection", tone: "warn" },
  basic: { label: "Basic protection", tone: "warn" },
  unsupported: { label: "iPhone: Apple's Lost Mode needed", tone: "danger" },
};

/**
 * Stolen protection for one phone: setting it up (USB once, then a yes on the phone),
 * turning stolen mode on with a message, and unlocking a phone that's offline.
 * Only this computer, the phone's owner, can do any of it.
 */
export function StolenSheet({ peer, onClose }: { peer: PeerSnapshot | null; onClose: () => void }) {
  return (
    <Sheet isOpen={!!peer} onClose={onClose} label={`Stolen protection for ${peer?.info.name ?? ""}`}>
      {peer && <StolenBody peer={peer} />}
    </Sheet>
  );
}

function StolenBody({ peer }: { peer: PeerSnapshot }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const id = peer.info.id;
  const [entry, setEntry] = useState<GuardEntry | null>(null);
  const [waitingYes, setWaitingYes] = useState(false);

  useEffect(() => {
    const load = () =>
      void bridge.guardStatuses().then(
        (list) => setEntry(list.find((e) => e.device === id) ?? { device: id, owns: false, status: null }),
        () => {},
      );
    load();
    return bridge.onEvent((e) => {
      if (e.type === "guard_status_received" && e.from === id) {
        setWaitingYes(false);
        load();
      }
    });
  }, [bridge, id]);

  if (!entry) return null;
  const owner = entry.owns && entry.status?.owner;
  if (entry.status?.other_owner && !owner) {
    return (
      <div className={s.body}>
        <h2 className={s.title}>Another computer protects {peer.info.name}</h2>
        <p className={p.itemHint}>
          Only the computer that set up stolen protection can use it. Turn it off there first.
        </p>
      </div>
    );
  }
  if (!owner) {
    return (
      <Setup
        peer={peer}
        waiting={waitingYes}
        onClaim={async () => {
          try {
            await bridge.guardClaim(id);
            setWaitingYes(true);
          } catch (e) {
            toast(err(e), "danger");
          }
        }}
      />
    );
  }
  return <Control peer={peer} entry={entry} />;
}

/** First time: the optional USB step, then a yes on the phone. */
function Setup({ peer, waiting, onClaim }: { peer: PeerSnapshot; waiting: boolean; onClaim: () => void }) {
  const bridge = useBridge();
  const [check, setCheck] = useState<GuardSetupCheck | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const android = peer.info.platform === "android";

  const look = async () => {
    setBusy(true);
    try {
      setCheck(await bridge.guardSetupCheck());
    } finally {
      setBusy(false);
    }
  };
  const run = async (serial: string) => {
    setBusy(true);
    try {
      setResult({ ok: true, text: await bridge.guardSetupRun(serial) });
    } catch (e) {
      setResult({ ok: false, text: err(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={s.body}>
      <div className={s.hero}>
        <span className={s.heroIcon}>
          <Icon name="shield" size={28} />
        </span>
        <div>
          <h2 className={s.title}>Protect {peer.info.name} if it's stolen</h2>
          <p className={p.itemHint}>
            When you turn stolen mode on from this computer, the phone shows a black screen with your message
            and sends you its location every 30 seconds. Only this computer can turn it on or off.
          </p>
        </div>
      </div>

      {android ? (
        <>
          <ol className={s.steps}>
            <li className={s.step}>
              <h3 className={s.stepTitle}>
                Full protection <span className={s.recommended}>recommended</span>
              </h3>
              <p className={p.itemHint}>
                Blocks the power menu, USB connections, safe mode and factory reset while it's stolen, and
                keeps Wi‑Fi and location on. Android allows this once, over a USB cable:
              </p>
              <ol className={s.howto}>
                <li>
                  On the phone, turn on <b>USB debugging</b>: Settings → About phone → tap <b>Build number</b>{" "}
                  7 times, then Settings → Developer options → USB debugging.
                </li>
                <li>
                  Remove every account for a minute: Settings → <b>Accounts</b> (Google, Samsung…). Android
                  requires it. You add them back right after.
                </li>
                <li>Plug the phone into this computer and allow USB debugging on the phone.</li>
              </ol>
              <div className={p.row}>
                <Button size="sm" icon="signal" isDisabled={busy} onPress={() => void look()}>
                  {check ? "Check again" : "Find the phone"}
                </Button>
              </div>
              {check && !check.adb && (
                <p className={s.note}>Android's adb tool isn't installed. {check.install_hint}</p>
              )}
              {check?.adb && check.phones.length === 0 && (
                <p className={s.note}>
                  No phone found over USB. Check the cable and that USB debugging is on.
                </p>
              )}
              {check?.phones.map((ph) => (
                <div key={ph.serial} className={s.usbPhone}>
                  <Icon name="phone" size={18} />
                  <span className={s.usbName}>{ph.model}</span>
                  {ph.state === "device" ? (
                    <Button size="sm" variant="primary" isDisabled={busy} onPress={() => void run(ph.serial)}>
                      Make Devti the protector
                    </Button>
                  ) : (
                    <span className={s.note}>
                      {ph.state === "unauthorized"
                        ? "Tap Allow on the phone's “Allow USB debugging?” question."
                        : `Not ready (${ph.state}).`}
                    </span>
                  )}
                </div>
              ))}
              {result && (
                <p className={s.result} data-ok={result.ok} role="status">
                  {result.text}
                </p>
              )}
            </li>
            <li className={s.step}>
              <h3 className={s.stepTitle}>Confirm on the phone</h3>
              <p className={p.itemHint}>
                {peer.info.name} asks once whether this computer may protect it. Without the USB step it still
                gets the stolen screen and live location, but a thief could use the power menu.
              </p>
              <ClaimButton peer={peer} waiting={waiting} onClaim={onClaim} />
            </li>
          </ol>
        </>
      ) : (
        <div className={s.step}>
          <p className={p.itemHint}>
            Apple doesn't let any app lock an iPhone, block its buttons or its USB port. Devti can show a lost
            screen and share its location while Devti is open on it. For real protection, use Apple's{" "}
            <b>Lost Mode</b> at icloud.com/find, which also works when the iPhone is off.
          </p>
          <ClaimButton peer={peer} waiting={waiting} onClaim={onClaim} />
        </div>
      )}
    </div>
  );
}

function ClaimButton({
  peer,
  waiting,
  onClaim,
}: {
  peer: PeerSnapshot;
  waiting: boolean;
  onClaim: () => void;
}) {
  return waiting ? (
    <p className={s.note} role="status">
      Waiting for you to answer on {peer.info.name}…
    </p>
  ) : (
    <Button variant="primary" icon="shield" isDisabled={!peer.online} onPress={onClaim}>
      {peer.online ? "Ask the phone" : `${peer.info.name} is offline`}
    </Button>
  );
}

/** This computer is the owner: status, message, on/off, offline unlock. */
function Control({ peer, entry }: { peer: PeerSnapshot; entry: GuardEntry }) {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const status = entry.status!;
  const st = status.state;
  const [message, setMessage] = useState(st.message || DEFAULT_MESSAGE);
  const [contact, setContact] = useState(st.contact);
  const [simLock, setSimLock] = useState(st.sim_lock);
  const [confirm, setConfirm] = useState(false);
  const [challenge, setChallenge] = useState("");
  const [code, setCode] = useState<string | null>(null);
  const level = LEVEL[status.report.level] ?? LEVEL.basic!;

  const send = async (on: boolean) => {
    const next: GuardState = { on, message, contact, sim_lock: simLock, reason: "", since: 0 };
    try {
      await bridge.guardSet(peer.info.id, next);
      toast(
        on ? `Stolen mode is on for ${peer.info.name}.` : `Stolen mode is off for ${peer.info.name}.`,
        on ? "danger" : "ok",
      );
    } catch (e) {
      toast(err(e), "danger");
    }
  };

  return (
    <div className={s.body}>
      <div className={s.hero} data-on={st.on || undefined}>
        <span className={s.heroIcon}>
          <Icon name={st.on ? "warning" : "shield"} size={28} />
        </span>
        <div>
          <h2 className={s.title}>
            {st.on ? `Stolen mode is on for ${peer.info.name}` : `${peer.info.name} is protected`}
          </h2>
          <p className={p.itemHint}>
            {st.on
              ? `Since ${relativeTime(st.since)}${st.reason === "sim_removed" ? ", because its SIM was removed" : ""}. Its live location shows on the map.`
              : "Only this computer can turn stolen mode on or off."}{" "}
            <span className={s.level} data-tone={level.tone}>
              {level.label}
            </span>
          </p>
        </div>
      </div>

      <div className={s.lists}>
        {status.report.protections.length > 0 && (
          <ul className={s.list} aria-label="Protected">
            {status.report.protections.map((x) => (
              <li key={x}>
                <Icon name="check" size={14} /> {x}
              </li>
            ))}
          </ul>
        )}
        {status.report.missing.length > 0 && (
          <ul className={s.list} data-missing aria-label="Not protected">
            {status.report.missing.map((x) => (
              <li key={x}>
                <Icon name="close" size={14} /> {x}
              </li>
            ))}
          </ul>
        )}
      </div>

      {!st.on ? (
        <>
          <Field label="Message on the stolen screen" value={message} onChange={setMessage} maxLength={300} />
          <Field
            label="How to reach you (optional)"
            value={contact}
            onChange={setContact}
            placeholder="+92 300 0000000"
            hint="Shown under the message, so an honest finder can return it."
            maxLength={120}
          />
          <div className={p.item}>
            <div className={p.itemText}>
              <span className={p.itemTitle}>Lock it if the SIM card is removed</span>
              <span className={p.itemHint}>
                Turns stolen mode on by itself the moment someone takes the SIM out.
              </span>
            </div>
            <Switch isSelected={simLock} onChange={setSimLock}>
              <span className="visually-hidden">Lock if the SIM is removed</span>
            </Switch>
          </div>
          {confirm ? (
            <div className={s.confirm} role="alertdialog" aria-label="Turn on stolen mode?">
              <p>
                {peer.info.name} will lock to a black screen right away. Only this computer, or its unlock
                code, can turn it off.
              </p>
              <div className={p.row}>
                <Button variant="ghost" onPress={() => setConfirm(false)}>
                  Cancel
                </Button>
                <Button
                  variant="danger"
                  icon="lock"
                  onPress={() => {
                    setConfirm(false);
                    void send(true);
                  }}
                >
                  Lock it now
                </Button>
              </div>
            </div>
          ) : (
            <div className={p.row}>
              <Button variant="danger" icon="lock" isDisabled={!peer.online} onPress={() => setConfirm(true)}>
                {peer.online ? "Turn on stolen mode" : `${peer.info.name} is offline`}
              </Button>
              <Button
                variant="ghost"
                isDisabled={!peer.online}
                onPress={() => void send(false).then(() => toast("Settings saved on the phone.", "ok"))}
              >
                Save settings
              </Button>
            </div>
          )}
          <Button
            size="sm"
            variant="ghost"
            onPress={() =>
              void bridge
                .guardRelease(peer.info.id)
                .then(() => toast(`This computer no longer protects ${peer.info.name}.`, "info"))
                .catch((e) => toast(err(e), "danger"))
            }
          >
            Stop protecting this phone
          </Button>
        </>
      ) : (
        <>
          <div className={p.row}>
            <Button variant="primary" icon="check" isDisabled={!peer.online} onPress={() => void send(false)}>
              {peer.online ? "Turn off stolen mode" : `${peer.info.name} is offline`}
            </Button>
          </div>
          <div className={s.unlock}>
            <h3 className={s.stepTitle}>Phone offline? Unlock it with a code</h3>
            <p className={p.itemHint}>
              The stolen screen shows an 8-digit question. Type it here and enter the answer on the phone.
            </p>
            <Field
              label="Question on the phone"
              value={challenge}
              onChange={(v) => {
                setChallenge(v);
                setCode(null);
              }}
              inputMode="numeric"
              maxLength={12}
              onSubmit={() =>
                void bridge
                  .guardUnlockCode(peer.info.id, challenge)
                  .then(setCode)
                  .catch((e) => toast(err(e), "danger"))
              }
            />
            <Button
              size="sm"
              onPress={() =>
                void bridge
                  .guardUnlockCode(peer.info.id, challenge)
                  .then(setCode)
                  .catch((e) => toast(err(e), "danger"))
              }
            >
              Get the answer
            </Button>
            {code && (
              <p className={s.code} data-testid="unlock-code">
                <span className="num">
                  {code.slice(0, 5)} {code.slice(5)}
                </span>
              </p>
            )}
          </div>
        </>
      )}
    </div>
  );
}
