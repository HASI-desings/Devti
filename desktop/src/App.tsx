import { MotionConfig } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import s from "./App.module.css";
import { Prism } from "./design/Prism";
import { AccessoriesPage } from "./features/accessories/AccessoriesPage";
import { AutomationPage } from "./features/automation/AutomationPage";
import { BatteryPage } from "./features/battery/BatteryPage";
import { ControlPage } from "./features/control/ControlPage";
import { DevicePage } from "./features/device/DevicePage";
import { EcosystemPage } from "./features/ecosystem/EcosystemPage";
import { FilesPage } from "./features/files/FilesPage";
import { SendPicker } from "./features/files/SendPicker";
import { HelpPage } from "./features/help/HelpPage";
import { MessagesPage } from "./features/messages/MessagesPage";
import { MirrorPage } from "./features/mirror/MirrorPage";
import { NotificationsPage } from "./features/notifications/NotificationsPage";
import { Done, Onboarding } from "./features/onboarding/Onboarding";
import { PairPanel } from "./features/pairing/PairPanel";
import { PluginsPage } from "./features/plugins/PluginsPage";
import { ScreensPage } from "./features/screens/ScreensPage";
import { SecurityPage } from "./features/security/SecurityPage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { CallCard } from "./features/shell/CallCard";
import { PreviewBanner } from "./features/shell/PreviewBanner";
import { Rail } from "./features/shell/Rail";
import { RingOverlay } from "./features/shell/RingOverlay";
import { Tour } from "./features/shell/Tour";
import { useBridge } from "./lib/bridge-context";
import { trackIdle } from "./lib/idle";
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
    const takeSend = async () => {
      const paths = await bridge.takePendingSend();
      if (paths.length) useStore.getState().setSendPaths(paths);
    };
    const offUi = bridge.onUiAction((action) => {
      if (action === "pair") setPairOpen(true);
      if (action === "send") void takeSend();
    });
    // Files dropped on the window: straight to a device if dropped on one, otherwise ask.
    const offDrop = bridge.onFileDrop((paths, x, y) => {
      const target = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-device-id]");
      const id = target?.dataset.deviceId;
      if (id) {
        void bridge.sendFiles(id, paths).catch((e) => useStore.getState().toast(String(e), "danger"));
      } else {
        useStore.getState().setSendPaths(paths);
      }
    });
    void load().then(async () => {
      if ((await bridge.takeUiAction()) === "pair") setPairOpen(true);
      await takeSend();
      useStore.getState().setTransfers(await bridge.transfers());
    });
    return () => {
      off();
      offUi();
      offDrop();
    };
  }, [bridge, load]);

  // Theme and motion preferences live on <html> so every token follows.
  useEffect(() => {
    const html = document.documentElement;
    if (settings.theme === "dark" || settings.theme === "light") html.dataset.theme = settings.theme;
    else delete html.dataset.theme;
    html.dataset.reduceMotion = settings.motion === "reduced" ? "true" : "false";
  }, [settings.theme, settings.motion]);

  useEffect(() => trackIdle(), []);

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
        <PreviewBanner />
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
      <PreviewBanner />
      <div className={s.app}>
        <Rail onPair={() => setPairOpen(true)} />
        <main className={s.main} id="main">
          {page === "ecosystem" && <EcosystemPage onPair={() => setPairOpen(true)} />}
          {page === "device" && <DevicePage />}
          {page === "files" && <FilesPage />}
          {page === "notifications" && <NotificationsPage />}
          {page === "battery" && <BatteryPage />}
          {page === "control" && <ControlPage />}
          {page === "mirror" && <MirrorPage />}
          {page === "screens" && <ScreensPage />}
          {page === "accessories" && <AccessoriesPage />}
          {page === "messages" && <MessagesPage />}
          {page === "automation" && <AutomationPage />}
          {page === "plugins" && <PluginsPage />}
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

      <SendPicker />
      <RingOverlay />
      <CallCard />
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
