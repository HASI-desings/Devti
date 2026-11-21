import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { DiscoveredDevice } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
import { Prism } from "../../design/Prism";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { formatCode, kindLabel, platformLabel } from "../../lib/format";
import { useStore } from "../../state/store";
import { Button, Field, Segmented } from "../../ui";
import s from "./PairPanel.module.css";
import { Qr } from "./Qr";

type Method = "phone" | "computer";

const APK_URL = "https://github.com/hasi-desings/devti/releases/latest";

/**
 * The whole pairing journey on the Hub: show QR + code → someone connects →
 * compare codes → done. Used in the "Pair a device" sheet and in onboarding.
 */
export function PairPanel({ onFinished, compact }: { onFinished?: () => void; compact?: boolean }) {
  const bridge = useBridge();
  const pairing = useStore((st) => st.pairing);
  const window_ = useStore((st) => st.window);
  const setPairing = useStore((st) => st.setPairing);
  const voices = useStore((st) => st.voices);
  const [method, setMethod] = useState<Method>("phone");
  const [error, setError] = useState<string | null>(null);
  const opened = useRef(false);

  const open = async () => {
    setError(null);
    try {
      setPairing({ stage: "waiting" });
      await bridge.openPairingWindow();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  // Open the window when the panel appears; close it when the panel goes away
  // without a pairing having completed.
  // biome-ignore lint/correctness/useExhaustiveDependencies: opens the window once on mount, closes on unmount
  useEffect(() => {
    if (!opened.current) {
      opened.current = true;
      void open();
    }
    return () => {
      const st = useStore.getState();
      if (st.window) void bridge.closePairingWindow();
      if (st.pairing?.stage !== "done") st.setPairing(null);
    };
  }, []);

  const stage = pairing?.stage ?? "waiting";

  return (
    <div className={s.panel} data-compact={compact || undefined}>
      <AnimatePresence mode="wait" initial={false}>
        {stage === "waiting" && method === "phone" && (
          <Step key="qr">
            <div className={s.qrSide}>
              <div className={s.qrCard}>
                {window_ ? (
                  <Qr
                    text={window_.uri}
                    size={compact ? 196 : 224}
                    label="Pairing QR code. Scan it with the Devti app."
                  />
                ) : (
                  <div className={s.qrPlaceholder} aria-busy="true" />
                )}
              </div>
              {window_ && <Expiry until={window_.expires_at} />}
            </div>
            <div className={s.copy}>
              <h2 className={s.title}>Scan with your phone</h2>
              <ol className={s.steps}>
                <li>Open Devti on your Android phone or iPhone.</li>
                <li>Tap “Pair with a computer” and point the camera at this code.</li>
                <li>Check that both screens show the same six digits.</li>
              </ol>
              {error && <p className={s.error}>{error}</p>}
              <p className={s.fine}>
                Don't have the app yet? Get the Android APK from the{" "}
                <button type="button" className={s.link} onClick={() => void bridge.openExternal(APK_URL)}>
                  Devti releases page
                </button>
                .
              </p>
              <div className={s.switchRow}>
                <Segmented<Method>
                  label="What are you pairing?"
                  value={method}
                  onChange={setMethod}
                  options={[
                    { value: "phone", label: "Phone or tablet", icon: "phone" },
                    { value: "computer", label: "Another computer", icon: "laptop" },
                  ]}
                />
              </div>
            </div>
          </Step>
        )}

        {stage === "waiting" && method === "computer" && (
          <Step key="computer">
            <ComputerPairing code={window_?.code ?? null} onBack={() => setMethod("phone")} />
          </Step>
        )}

        {pairing?.stage === "connecting" && (
          <Step key="connecting" center>
            <Prism size={104} voices={[]} />
            <h2 className={s.title}>{pairing.peer ? `Connecting to ${pairing.peer.name}` : "Connecting"}</h2>
            <p className={s.lead}>Agreeing on a secret key. This takes a second.</p>
          </Step>
        )}

        {pairing?.stage === "verify" && (
          <Step key="verify" center>
            <p className={s.lead}>
              Does <strong>{pairing.peer.name}</strong> show this code?
            </p>
            <p className="visually-hidden">Verification code {pairing.sas.split("").join(" ")}</p>
            <div className={s.sas} aria-hidden="true" data-testid="sas">
              {formatCode(pairing.sas)
                .split("")
                .map((ch, i) => (
                  <motion.span
                    key={`${i}-${ch}`}
                    initial={{ opacity: 0, y: 14 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ delay: i * 0.05, type: "spring", stiffness: 320, damping: 24 }}
                    className={ch === " " ? s.sasGap : s.sasDigit}
                  >
                    {ch === " " ? "" : ch}
                  </motion.span>
                ))}
            </div>
            {pairing.confirmed ? (
              <p className={s.waiting}>Waiting for {pairing.peer.name} to confirm…</p>
            ) : (
              <div className={s.actions}>
                <Button
                  variant="primary"
                  size="lg"
                  icon="check"
                  onPress={async () => {
                    setPairing({ ...pairing, confirmed: true });
                    try {
                      await bridge.confirmPairing(pairing.pairingId, true);
                    } catch (e) {
                      setPairing({ stage: "failed", reason: "other", message: String(e) });
                    }
                  }}
                >
                  The codes match
                </Button>
                <Button
                  variant="quiet"
                  size="lg"
                  onPress={() => void bridge.confirmPairing(pairing.pairingId, false).catch(() => {})}
                >
                  They're different
                </Button>
              </div>
            )}
            <p className={s.fine}>Matching codes prove nobody is listening in between the two devices.</p>
          </Step>
        )}

        {pairing?.stage === "done" && (
          <Step key="done" center>
            <motion.div
              className={s.joined}
              style={{ color: voiceVar(voices[pairing.peer.info.id] ?? "iris") }}
              initial={{ scale: 0.4, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={{ type: "spring", stiffness: 220, damping: 16 }}
            >
              <Icon name={deviceIcon(pairing.peer.info.kind)} size={44} />
            </motion.div>
            <h2 className={s.title}>{pairing.peer.info.name} joined your ecosystem</h2>
            <p className={s.lead}>It reconnects by itself whenever both devices are on the same network.</p>
            <div className={s.actions}>
              {onFinished && (
                <Button variant="primary" size="lg" onPress={onFinished}>
                  Continue
                </Button>
              )}
              <Button
                variant="quiet"
                size="lg"
                icon="plus"
                onPress={() => {
                  void open();
                }}
              >
                Pair another device
              </Button>
            </div>
          </Step>
        )}

        {pairing?.stage === "failed" && (
          <Step key="failed" center>
            <span className={s.failIcon}>
              <Icon name="warning" size={34} />
            </span>
            <h2 className={s.title}>Pairing didn't finish</h2>
            <p className={s.lead}>{pairing.message}</p>
            <div className={s.actions}>
              <Button variant="primary" size="lg" onPress={() => void open()}>
                Try again
              </Button>
            </div>
          </Step>
        )}
      </AnimatePresence>
    </div>
  );
}

function Step({ children, center }: { children: React.ReactNode; center?: boolean }) {
  return (
    <motion.div
      className={center ? s.stepCenter : s.step}
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
    >
      {children}
    </motion.div>
  );
}

function Expiry({ until }: { until: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const left = Math.max(0, Math.round((until - now) / 1000));
  const m = Math.floor(left / 60);
  const sec = String(left % 60).padStart(2, "0");
  return (
    <p className={s.expiry}>
      {left > 0 ? (
        <>
          Code refreshes in <span className="num">{`${m}:${sec}`}</span>
        </>
      ) : (
        "This code expired."
      )}
    </p>
  );
}

function ComputerPairing({ code, onBack }: { code: string | null; onBack: () => void }) {
  const bridge = useBridge();
  const nearbyMap = useStore((st) => st.nearby);
  const nearby = useMemo(() => Object.values(nearbyMap).filter((d) => d.pairing_open), [nearbyMap]);
  const [target, setTarget] = useState<DiscoveredDevice | "manual" | null>(null);
  const [entered, setEntered] = useState("");
  const [address, setAddress] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setError(null);
    const digits = entered.replace(/\D/g, "");
    if (digits.length !== 6) {
      setError("Enter the 6-digit code shown on the other computer.");
      return;
    }
    try {
      if (target === "manual") {
        if (!/^[\w.:[\]-]+:\d+$/.test(address.trim())) {
          setError("Enter the address as shown on the other computer, like 192.168.1.20:47810.");
          return;
        }
        await bridge.pairWithCodeAt(address.trim(), digits);
      } else if (target) {
        await bridge.pairWithCode(target.id, digits);
      }
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  return (
    <div className={s.computer}>
      <div className={s.codeSide}>
        <p className={s.label}>To add this computer from another one, enter</p>
        <p className={s.bigCode} data-testid="pair-code">
          {code ?? "— — —"}
        </p>
        <p className={s.fine}>The code works for 5 minutes and only on your network.</p>
      </div>
      <div className={s.copy}>
        <h2 className={s.title}>Or join another computer</h2>
        {!target && (
          <>
            {nearby.length === 0 ? (
              <p className={s.lead}>
                No computer nearby is ready to pair. On the other computer, open Devti and choose “Pair a
                device”.
              </p>
            ) : (
              <ul className={s.nearby}>
                {nearby.map((d) => (
                  <li key={d.id}>
                    <button type="button" className={s.nearbyRow} onClick={() => setTarget(d)}>
                      <Icon name={deviceIcon(d.kind)} size={22} />
                      <span className={s.nearbyName}>{d.name}</span>
                      <span className={s.nearbyMeta}>
                        {kindLabel(d.kind)}, {platformLabel(d.platform)}
                      </span>
                      <Icon name="chevronRight" size={18} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <Button variant="ghost" size="sm" onPress={() => setTarget("manual")}>
              Enter an address instead
            </Button>
          </>
        )}
        {target && (
          <div className={s.codeForm}>
            <p className={s.lead}>
              {target === "manual"
                ? "Type the address and code shown on the other computer."
                : `Type the code shown on ${target.name}.`}
            </p>
            {target === "manual" && (
              <Field label="Address" value={address} onChange={setAddress} placeholder="192.168.1.20:47810" />
            )}
            <Field
              label="Pairing code"
              value={entered}
              onChange={(v) => setEntered(v.replace(/[^\d ]/g, "").slice(0, 7))}
              inputMode="numeric"
              placeholder="000 000"
              autoFocus={target !== "manual"}
              error={error}
              onSubmit={() => void submit()}
            />
            <div className={s.actions}>
              <Button variant="primary" onPress={() => void submit()}>
                Pair
              </Button>
              <Button variant="ghost" onPress={() => setTarget(null)}>
                Back
              </Button>
            </div>
          </div>
        )}
        <div className={s.switchRow}>
          <Button variant="ghost" size="sm" icon="chevronLeft" onPress={onBack}>
            Pair a phone or tablet
          </Button>
        </div>
      </div>
    </div>
  );
}
