import { MotionConfig } from "motion/react";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import s from "./App.module.css";
import { Prism } from "./design/Prism";
import { EcosystemPage } from "./features/ecosystem/EcosystemPage";
import { SendPicker } from "./features/files/SendPicker";
import { PairPanel } from "./features/pairing/PairPanel";
import { CallCard } from "./features/shell/CallCard";
import { PreviewBanner } from "./features/shell/PreviewBanner";
import { Rail } from "./features/shell/Rail";
import { RingOverlay } from "./features/shell/RingOverlay";
import { Tour } from "./features/shell/Tour";
import { useBridge } from "./lib/bridge-context";
import { trackIdle } from "./lib/idle";
import { useStore } from "./state/store";
import { Button, Sheet, Toasts } from "./ui";

// Pages load when first opened, so an idle Hub keeps only the dashboard in memory.
const AccessoriesPage = lazy(() =>
  import("./features/accessories/AccessoriesPage").then((m) => ({ default: m.AccessoriesPage })),
);
const AutomationPage = lazy(() =>
  import("./features/automation/AutomationPage").then((m) => ({ default: m.AutomationPage })),
);
const BatteryPage = lazy(() =>
  import("./features/battery/BatteryPage").then((m) => ({ default: m.BatteryPage })),
);
const ControlPage = lazy(() =>
  import("./features/control/ControlPage").then((m) => ({ default: m.ControlPage })),
);
const DevicePage = lazy(() =>
  import("./features/device/DevicePage").then((m) => ({ default: m.DevicePage })),
);
const FilesPage = lazy(() => import("./features/files/FilesPage").then((m) => ({ default: m.FilesPage })));
const HelpPage = lazy(() => import("./features/help/HelpPage").then((m) => ({ default: m.HelpPage })));
const MessagesPage = lazy(() =>
  import("./features/messages/MessagesPage").then((m) => ({ default: m.MessagesPage })),
);
const MirrorPage = lazy(() =>
  import("./features/mirror/MirrorPage").then((m) => ({ default: m.MirrorPage })),
);
const NotificationsPage = lazy(() =>
  import("./features/notifications/NotificationsPage").then((m) => ({ default: m.NotificationsPage })),
);
const PluginsPage = lazy(() =>
  import("./features/plugins/PluginsPage").then((m) => ({ default: m.PluginsPage })),
);
const ScreensPage = lazy(() =>
  import("./features/screens/ScreensPage").then((m) => ({ default: m.ScreensPage })),
);
const SecurityPage = lazy(() =>
  import("./features/security/SecurityPage").then((m) => ({ default: m.SecurityPage })),
);
const SettingsPage = lazy(() =>
  import("./features/settings/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);
const Onboarding = lazy(() =>
  import("./features/onboarding/Onboarding").then((m) => ({ default: m.Onboarding })),
);
const Done = lazy(() => import("./features/onboarding/Onboarding").then((m) => ({ default: m.Done })));

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
        <Suspense fallback={null}>
          <Onboarding
            onFinish={finishOnboarding}
            onFinishWithTour={() => {
              finishOnboarding();
              setDoneAfterTour(true);
              navigate("ecosystem");
              setTour(true);
            }}
          />
        </Suspense>
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
          <Suspense fallback={null}>
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
          </Suspense>
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
          <Suspense fallback={null}>
            <Done
              onFinish={(next) => {
                setDoneOpen(false);
                if (next === "pair") setPairOpen(true);
                if (next === "security") navigate("security");
              }}
            />
          </Suspense>
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
