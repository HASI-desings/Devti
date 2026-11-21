import { MotionConfig } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import s from "./App.module.css";
import { Prism } from "./design/Prism";
import { DevicePage } from "./features/device/DevicePage";
import { EcosystemPage } from "./features/ecosystem/EcosystemPage";
import { HelpPage } from "./features/help/HelpPage";
import { Done, Onboarding } from "./features/onboarding/Onboarding";
import { PairPanel } from "./features/pairing/PairPanel";
import { SecurityPage } from "./features/security/SecurityPage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { Rail } from "./features/shell/Rail";
import { Tour } from "./features/shell/Tour";
import { useBridge } from "./lib/bridge-context";
import { useStore } from "./state/store";
import { Button, Sheet, Toasts } from "./ui";

export function App() {
  const bridge = useBridge();
  const ready = useStore((st) => st.ready);
  const error = useStore((st) => st.error);
  const settings = useStore((st) => st.settings);
  const page = useStore((st) => st.page);
  const tour = useStore((st) => st.tour);
  const navigate = useStore((st) => st.navigate);
  const setSetting = useStore((st) => st.setSetting);
  const setTour = useStore((st) => st.setTour);
  const [pairOpen, setPairOpen] = useState(false);
  const [doneOpen, setDoneOpen] = useState(false);
  const [doneAfterTour, setDoneAfterTour] = useState(false);

  const load = useCallback(async () => {
    try {
      const snap = await bridge.snapshot();
      useStore.getState().load(snap, bridge.mode);
    } catch (e) {
      useStore.getState().fail(String(e instanceof Error ? e.message : e));
    }
  }, [bridge]);

  useEffect(() => {
    const off = bridge.onEvent((e) => {
      if (e.type === "resync") {
        void load();
        return;
      }
      useStore.getState().apply(e);
      // Someone started pairing with the Hub while the sheet was closed: bring it up.
      if (
        e.type === "pairing_started" &&
        e.role === "hub" &&
        useStore.getState().settings.onboarding_done === "true"
      ) {
        setPairOpen(true);
      }
    });
    const offUi = bridge.onUiAction((action) => {
      if (action === "pair") setPairOpen(true);
    });
    void load().then(async () => {
      if ((await bridge.takeUiAction()) === "pair") setPairOpen(true);
    });
    return () => {
      off();
      offUi();
    };
  }, [bridge, load]);

  // Theme and motion preferences live on <html> so every token follows.
  useEffect(() => {
    const html = document.documentElement;
    if (settings.theme === "dark" || settings.theme === "light") html.dataset.theme = settings.theme;
    else delete html.dataset.theme;
    html.dataset.reduceMotion = settings.motion === "reduced" ? "true" : "false";
  }, [settings.theme, settings.motion]);

  useEffect(() => {
    const onVis = () => {
      document.documentElement.dataset.hidden = String(document.hidden);
    };
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  }, []);

  const finishOnboarding = (next?: "pair" | "security") => {
    setSetting("onboarding_done", "true");
    void bridge.setSetting("onboarding_done", "true");
    if (next === "pair") setPairOpen(true);
    if (next === "security") navigate("security");
  };

  const reducedMotion =
    settings.motion === "reduced" ? "always" : settings.motion === "full" ? "never" : "user";

  if (error) {
    return (
      <div className={s.center}>
        <Prism size={96} voices={[]} glow={false} />
        <h1 className={s.centerTitle}>Devti couldn't start</h1>
        <p className={s.centerText}>{error}</p>
        <Button variant="primary" onPress={() => void load()}>
          Try again
        </Button>
      </div>
    );
  }

  if (!ready) {
    return (
      <div className={s.center} aria-busy="true">
        <Prism size={96} voices={[]} />
        <p className={s.centerText}>Starting Devti…</p>
      </div>
    );
  }

  if (settings.onboarding_done !== "true") {
    return (
      <MotionConfig reducedMotion={reducedMotion}>
        <Onboarding
          onFinish={finishOnboarding}
          onFinishWithTour={() => {
            finishOnboarding();
            setDoneAfterTour(true);
            navigate("ecosystem");
            setTour(true);
          }}
        />
        <Toasts />
      </MotionConfig>
    );
  }

  return (
    <MotionConfig reducedMotion={reducedMotion}>
      <div className={s.app}>
        <Rail onPair={() => setPairOpen(true)} />
        <main className={s.main} id="main">
          {page === "ecosystem" && <EcosystemPage onPair={() => setPairOpen(true)} />}
          {page === "device" && <DevicePage />}
          {page === "security" && <SecurityPage onPair={() => setPairOpen(true)} />}
          {page === "settings" && <SettingsPage />}
          {page === "help" && (
            <HelpPage
              onRerunSetup={() => {
                setSetting("onboarding_done", "false");
                void bridge.setSetting("onboarding_done", "false");
              }}
            />
          )}
        </main>
      </div>

      <Sheet isOpen={pairOpen} onClose={() => setPairOpen(false)} label="Pair a device">
        <div className={s.sheetHead}>
          <Button variant="ghost" size="sm" iconOnly icon="close" onPress={() => setPairOpen(false)}>
            Close
          </Button>
        </div>
        <PairPanel onFinished={() => setPairOpen(false)} />
      </Sheet>

      <Sheet isOpen={doneOpen} onClose={() => setDoneOpen(false)} label="Devti is ready">
        <div className={s.doneSheet}>
          <Done
            onFinish={(next) => {
              setDoneOpen(false);
              if (next === "pair") setPairOpen(true);
              if (next === "security") navigate("security");
            }}
          />
        </div>
      </Sheet>

      {tour && (
        <Tour
          onDone={() => {
            if (doneAfterTour) {
              setDoneAfterTour(false);
              setDoneOpen(true);
            }
          }}
        />
      )}
      <Toasts />
    </MotionConfig>
  );
}
