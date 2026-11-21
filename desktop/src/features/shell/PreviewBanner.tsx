import { Icon } from "../../design/icons";
import { useBridge } from "../../lib/bridge-context";
import s from "./PreviewBanner.module.css";

const INSTALL_URL = "https://github.com/hasi-desings/devti#install";

/**
 * Shown on every screen when the UI runs in a plain web browser. Browsers can't open the
 * encrypted QUIC connections Devti uses, so nothing here talks to real devices.
 */
export function PreviewBanner() {
  const bridge = useBridge();
  if (bridge.mode !== "preview") return null;
  return (
    <div className={s.banner} role="note">
      <Icon name="info" size={18} />
      <p>
        <strong>Browser preview.</strong> Every device here is simulated. To pair your real phone and
        computers, install the Devti desktop app.
      </p>
      <button type="button" className={s.link} onClick={() => void bridge.openExternal(INSTALL_URL)}>
        How to install
      </button>
    </div>
  );
}
