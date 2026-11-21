import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { type ReactNode, useEffect, useState } from "react";
import type { BluetoothDevice, SystemInfo } from "../../bridge";
import { Icon, type IconName } from "../../design/icons";
import { Prism } from "../../design/Prism";
import { VOICES, voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { useStore } from "../../state/store";
import { Button, Field, Switch } from "../../ui";
import { PairPanel } from "../pairing/PairPanel";
import s from "./Onboarding.module.css";

export const STEPS = ["welcome", "permissions", "identity", "pair", "bluetooth", "tour", "done"] as const;
type Step = (typeof STEPS)[number];
const STEP_LABEL: Record<Step, string> = {
  welcome: "Welcome",
  permissions: "Permissions",
  identity: "This computer",
  pair: "First device",
  bluetooth: "Accessories",
  tour: "Tour",
  done: "Ready",
};

export function Onboarding({
  onFinish,
  onFinishWithTour,
}: {
  onFinish: (next?: "pair" | "security") => void;
  onFinishWithTour: () => void;
}) {
  const [step, setStep] = useState<Step>("welcome");
  const [role, setRole] = useState<"hub" | "join">("hub");
  const idx = STEPS.indexOf(step);
  const next = () => setStep(STEPS[Math.min(idx + 1, STEPS.length - 1)]!);
  const back = () => setStep(STEPS[Math.max(idx - 1, 0)]!);

  return (
    <div className={s.shell}>
      <Progress current={idx} />
      <main className={s.stage}>
        <AnimatePresence mode="wait">
          <motion.section
            key={step}
            className={s.step}
            initial={{ opacity: 0, y: 18 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }}
            transition={{ duration: 0.32, ease: [0.22, 1, 0.36, 1] }}
            aria-label={STEP_LABEL[step]}
          >
            {step === "welcome" && <Welcome onNext={next} />}
            {step === "permissions" && <Permissions onNext={next} onBack={back} />}
            {step === "identity" && <Identity role={role} setRole={setRole} onNext={next} onBack={back} />}
            {step === "pair" && <PairStep role={role} onNext={next} onBack={back} />}
            {step === "bluetooth" && <Bluetooth onNext={next} onBack={back} />}
            {step === "tour" && <Tour onTour={onFinishWithTour} onSkip={next} />}
            {step === "done" && <Done onFinish={onFinish} />}
          </motion.section>
        </AnimatePresence>
      </main>
    </div>
  );
}

function Progress({ current }: { current: number }) {
  return (
    <nav className={s.progress} aria-label="Setup progress">
      <ol>
        {STEPS.map((st, i) => (
          <li
            key={st}
            data-state={i < current ? "done" : i === current ? "current" : "todo"}
            aria-current={i === current ? "step" : undefined}
          >
            <span
              className={s.dot}
              style={{ "--voice": voiceVar(VOICES[i % VOICES.length]!) } as React.CSSProperties}
            />
            <span className={s.stepName}>{STEP_LABEL[st]}</span>
          </li>
        ))}
      </ol>
      <div className={s.track} aria-hidden>
        <motion.div
          className={s.fill}
          animate={{ scaleX: current / (STEPS.length - 1) }}
          transition={{ type: "spring", stiffness: 80, damping: 20 }}
        />
      </div>
    </nav>
  );
}

function Nav({ onBack, children }: { onBack?: () => void; children: ReactNode }) {
  return (
    <div className={s.nav}>
      {onBack ? (
        <Button variant="ghost" icon="chevronLeft" onPress={onBack}>
          Back
        </Button>
      ) : (
        <span />
      )}
      <div className={s.navRight}>{children}</div>
    </div>
  );
}

/* ------------------------------------------------------------------ Welcome */

function Welcome({ onNext }: { onNext: () => void }) {
  const reduce = useReducedMotion();
  const [lit, setLit] = useState(reduce ? 6 : 0);
  useEffect(() => {
    if (reduce) return;
    const timers = VOICES.map((_, i) => setTimeout(() => setLit(i + 1), 900 + i * 260));
    return () => timers.forEach(clearTimeout);
  }, [reduce]);
  // Six voices start at the edges and travel into the prism.
  const starts = [
    [-320, -160],
    [300, -190],
    [360, 60],
    [220, 210],
    [-260, 200],
    [-380, 20],
  ];
  return (
    <div className={s.welcome}>
      <div className={s.converge} aria-hidden>
        {!reduce &&
          VOICES.map((v, i) => (
            <motion.span
              key={v}
              className={s.spark}
              style={{ background: voiceVar(v), boxShadow: `0 0 18px ${voiceVar(v)}` }}
              initial={{ x: starts[i]![0], y: starts[i]![1], opacity: 0, scale: 1.4 }}
              animate={{ x: 0, y: 8, opacity: [0, 1, 1, 0], scale: [1.4, 1, 0.6] }}
              transition={{ duration: 1.2, delay: 0.2 + i * 0.26, ease: [0.65, 0, 0.35, 1] }}
            />
          ))}
        <Prism size={168} voices={VOICES.slice(0, lit)} />
      </div>
      <h1 className={s.hero}>All your devices, one living system.</h1>
      <p className={s.lead}>
        Devti connects your computers, phones, tablets and accessories straight to each other. Share screens,
        files and control between them, privately and for free.
      </p>
      <Button variant="primary" size="lg" onPress={onNext} autoFocus>
        Set up Devti
      </Button>
      <p className={s.fine}>Takes about two minutes. Nothing leaves your network.</p>
    </div>
  );
}

/* -------------------------------------------------------------- Permissions */

function Permissions({ onNext, onBack }: { onNext: () => void; onBack: () => void }) {
  const bridge = useBridge();
  const [sys, setSys] = useState<SystemInfo | null>(null);
  useEffect(() => {
    void bridge.systemInfo().then(setSys);
  }, [bridge]);
  const win = sys?.os === "windows";
  const wayland = sys?.session === "wayland";

  const items: { icon: IconName; title: string; why: string; when: string; link?: [string, string] }[] = [
    {
      icon: "network",
      title: "Your local network",
      why: "Devti talks to your devices directly over Wi-Fi or Ethernet. Nothing goes through the internet.",
      when: win
        ? "When Windows Firewall asks, allow Devti on private networks."
        : "Your firewall must allow UDP port 47810 on your home network.",
    },
    {
      icon: "bluetooth",
      title: "Bluetooth",
      why: "To find your earbuds, speakers and other accessories and share them with every device.",
      when: "Used when you open the accessory step or the Bluetooth page.",
      link: win ? ["ms-settings:bluetooth", "Open Bluetooth settings"] : undefined,
    },
    {
      icon: "cast",
      title: "Screen capture",
      why: "So a phone or tablet can show or extend this screen.",
      when: wayland
        ? "On Wayland your desktop asks you each time a screen is shared. That's expected."
        : "Asked the first time you share this screen.",
    },
    {
      icon: "cursor",
      title: "Input control",
      why: "So your phone can work as a touchpad, keyboard or remote for this computer.",
      when: wayland
        ? "On Wayland your desktop shows a remote control prompt the first time."
        : "Only active while a device you trust is controlling this computer.",
    },
    {
      icon: "bell",
      title: "Notifications",
      why: "So Devti can tell you when a device wants to pair or needs your attention.",
      when: "Shown as normal system notifications.",
      link: win ? ["ms-settings:notifications", "Open notification settings"] : undefined,
    },
  ];

  return (
    <div className={s.wide}>
      <h1 className={s.title}>What Devti needs, and why</h1>
      <p className={s.lead}>
        Nothing is requested yet. Devti asks for each permission the first time you use the feature that needs
        it.
      </p>
      <ul className={s.perms}>
        {items.map((it) => (
          <li key={it.title}>
            <span className={s.permIcon}>
              <Icon name={it.icon} size={22} />
            </span>
            <div className={s.permText}>
              <span className={s.permTitle}>{it.title}</span>
              <span>{it.why}</span>
              <span className={s.permWhen}>{it.when}</span>
            </div>
            {it.link && (
              <Button
                variant="ghost"
                size="sm"
                icon="external"
                onPress={() => void bridge.openExternal(it.link![0])}
              >
                {it.link[1]}
              </Button>
            )}
          </li>
        ))}
      </ul>
      <Nav onBack={onBack}>
        <Button variant="primary" onPress={onNext}>
          Continue
        </Button>
      </Nav>
    </div>
  );
}

/* ----------------------------------------------------------------- Identity */

function Identity({
  role,
  setRole,
  onNext,
  onBack,
}: {
  role: "hub" | "join";
  setRole: (r: "hub" | "join") => void;
  onNext: () => void;
  onBack: () => void;
}) {
  const bridge = useBridge();
  const local = useStore((st) => st.local)!;
  const toast = useStore((st) => st.toast);
  const [name, setName] = useState(local.info.name);
  const save = async () => {
    try {
      if (name.trim() && name.trim() !== local.info.name) await bridge.rename(name);
      onNext();
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  };
  return (
    <div className={s.narrow}>
      <h1 className={s.title}>Name this computer</h1>
      <p className={s.lead}>Your other devices will see this name.</p>
      <Field
        label="Computer name"
        value={name}
        onChange={setName}
        maxLength={48}
        autoFocus
        onSubmit={() => void save()}
      />
      <fieldset className={s.roles}>
        <legend className={s.legend}>How will you use it?</legend>
        <RoleOption
          checked={role === "hub"}
          onSelect={() => setRole("hub")}
          icon="hub"
          title="Make it the Hub"
          text="Your main computer. Phones, tablets, accessories and other computers connect to it."
        />
        <RoleOption
          checked={role === "join"}
          onSelect={() => setRole("join")}
          icon="link"
          title="Join another Hub"
          text="You already set up Devti on another computer and want this one to join it."
        />
      </fieldset>
      <Nav onBack={onBack}>
        <Button variant="primary" onPress={() => void save()} isDisabled={!name.trim()}>
          Continue
        </Button>
      </Nav>
    </div>
  );
}

function RoleOption({
  checked,
  onSelect,
  icon,
  title,
  text,
}: {
  checked: boolean;
  onSelect: () => void;
  icon: IconName;
  title: string;
  text: string;
}) {
  return (
    <label className={s.role} data-checked={checked || undefined}>
      <input type="radio" name="role" checked={checked} onChange={onSelect} className="visually-hidden" />
      <Icon name={icon} size={26} />
      <span className={s.roleText}>
        <span className={s.roleTitle}>{title}</span>
        <span>{text}</span>
      </span>
      <span className={s.radio} aria-hidden />
    </label>
  );
}

/* --------------------------------------------------------------------- Pair */

function PairStep({
  role,
  onNext,
  onBack,
}: {
  role: "hub" | "join";
  onNext: () => void;
  onBack: () => void;
}) {
  const pairing = useStore((st) => st.pairing);
  return (
    <div className={s.wide}>
      <h1 className={s.title}>{role === "hub" ? "Pair your first device" : "Join your Hub"}</h1>
      <p className={s.lead}>
        {role === "hub"
          ? "Install Devti on your phone, then scan this code from the app."
          : "Choose “Another computer” below and enter the code your Hub shows under Pair a device."}
      </p>
      <div className={s.pairBox}>
        <PairPanel compact onFinished={onNext} />
      </div>
      <Nav onBack={onBack}>
        <Button variant={pairing?.stage === "done" ? "primary" : "ghost"} onPress={onNext}>
          {pairing?.stage === "done" ? "Continue" : "Skip for now"}
        </Button>
      </Nav>
    </div>
  );
}

/* ---------------------------------------------------------------- Bluetooth */

function Bluetooth({ onNext, onBack }: { onNext: () => void; onBack: () => void }) {
  const bridge = useBridge();
  const [devices, setDevices] = useState<BluetoothDevice[]>([]);
  const [adapter, setAdapter] = useState<{ available: boolean; message: string } | null>(null);
  useEffect(() => {
    let stop: (() => void) | undefined;
    let alive = true;
    void bridge
      .bluetoothScan((e) => {
        if (!alive) return;
        if (e.type === "adapter") setAdapter({ available: e.available, message: e.message });
        if (e.type === "device")
          setDevices((d) => {
            const rest = d.filter((x) => x.id !== e.device.id);
            return [...rest, e.device].sort((a, b) => (b.rssi ?? -200) - (a.rssi ?? -200));
          });
      })
      .then((fn) => {
        if (alive) stop = fn;
        else fn();
      })
      .catch((err) => setAdapter({ available: false, message: String(err) }));
    return () => {
      alive = false;
      stop?.();
    };
  }, [bridge]);

  return (
    <div className={s.wide}>
      <h1 className={s.title}>Accessories nearby</h1>
      <p className={s.lead}>
        Put your earbuds, headphones or speaker in pairing mode. Devti is listening for them now.
      </p>
      <div className={s.scan}>
        <div className={s.radar} data-active={adapter?.available || undefined} aria-hidden>
          <Icon name="bluetooth" size={30} />
        </div>
        <div className={s.scanList} aria-live="polite">
          {adapter && !adapter.available && <p className={s.scanNote}>{adapter.message}</p>}
          {adapter?.available && devices.length === 0 && (
            <p className={s.scanNote}>Looking for accessories…</p>
          )}
          <ul>
            <AnimatePresence>
              {devices.map((d) => (
                <motion.li
                  key={d.id}
                  initial={{ opacity: 0, x: -10 }}
                  animate={{ opacity: 1, x: 0 }}
                  className={s.btRow}
                >
                  <Icon name="accessory" size={22} />
                  <span className={s.btText}>
                    <span className={s.permTitle}>{d.name ?? "Unnamed accessory"}</span>
                    <span className={s.permWhen}>
                      {[d.manufacturer, ...d.services].filter(Boolean).join(", ") || "Bluetooth Low Energy"}
                    </span>
                  </span>
                  <Signal rssi={d.rssi} />
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        </div>
      </div>
      <p className={s.fine}>
        Devti shows what it detects here. Pairing accessories and controlling their features (noise
        cancelling, equalizer, per-bud battery) arrives with Devti's Bluetooth module in a later update.
      </p>
      <Nav onBack={onBack}>
        <Button variant="primary" onPress={onNext}>
          Continue
        </Button>
      </Nav>
    </div>
  );
}

function Signal({ rssi }: { rssi: number | null }) {
  const bars = rssi == null ? 0 : rssi > -55 ? 4 : rssi > -67 ? 3 : rssi > -80 ? 2 : 1;
  return (
    <span className={s.signal} role="img" aria-label={`Signal ${bars} of 4`}>
      {[1, 2, 3, 4].map((b) => (
        <span key={b} data-on={b <= bars || undefined} style={{ height: 4 + b * 3 }} />
      ))}
    </span>
  );
}

/* --------------------------------------------------------------------- Tour */

function Tour({ onTour, onSkip }: { onTour: () => void; onSkip: () => void }) {
  return (
    <div className={s.narrow}>
      <span className={s.tourIcon}>
        <Icon name="sparkle" size={34} />
      </span>
      <h1 className={s.title}>Want a quick look around?</h1>
      <p className={s.lead}>
        Five short stops through the ecosystem map, your devices and the safety controls.
      </p>
      <div className={s.navRight}>
        <Button variant="primary" size="lg" onPress={onTour} autoFocus>
          Show me around
        </Button>
        <Button variant="ghost" size="lg" onPress={onSkip}>
          Skip the tour
        </Button>
      </div>
    </div>
  );
}

/* --------------------------------------------------------------------- Done */

export function Done({ onFinish }: { onFinish: (next?: "pair" | "security") => void }) {
  const bridge = useBridge();
  const settings = useStore((st) => st.settings);
  const setSetting = useStore((st) => st.setSetting);
  const [auto, setAuto] = useState(settings.autostart === "true");
  return (
    <div className={s.wide}>
      <h1 className={s.title}>Devti is ready</h1>
      <p className={s.lead}>Three good places to start.</p>
      <div className={s.suggest}>
        <button type="button" className={s.card} onClick={() => onFinish("pair")}>
          <Icon name="link" size={26} />
          <span className={s.permTitle}>Pair another device</span>
          <span>Add a phone, tablet or second computer.</span>
        </button>
        <button type="button" className={s.card} onClick={() => onFinish("security")}>
          <Icon name="shield" size={26} />
          <span className={s.permTitle}>Choose what each device may do</span>
          <span>Trusted or restricted, one switch per feature.</span>
        </button>
        <div className={s.card}>
          <Icon name="power" size={26} />
          <span className={s.permTitle}>Start with your computer</span>
          <span>So your devices reconnect on their own.</span>
          <Switch
            isSelected={auto}
            onChange={async (v) => {
              try {
                const on = await bridge.setAutostart(v);
                setAuto(on);
                setSetting("autostart", String(on));
              } catch {
                setAuto(false);
              }
            }}
          >
            {auto ? "On" : "Off"}
          </Switch>
        </div>
      </div>
      <Nav>
        <Button variant="primary" size="lg" onPress={() => onFinish()}>
          Open my ecosystem
        </Button>
      </Nav>
    </div>
  );
}
