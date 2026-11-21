import { Icon, type IconName } from "../../design/icons";
import { Logo } from "../../design/Prism";
import { type Page, useStore } from "../../state/store";
import s from "./Rail.module.css";

const ITEMS: { page: Page; label: string; icon: IconName }[] = [
  { page: "ecosystem", label: "Ecosystem", icon: "ecosystem" },
  { page: "security", label: "Security", icon: "shield" },
  { page: "settings", label: "Settings", icon: "settings" },
  { page: "help", label: "Help", icon: "help" },
];

export function Rail({ onPair }: { onPair: () => void }) {
  const page = useStore((st) => st.page);
  const navigate = useStore((st) => st.navigate);
  const paused = useStore((st) => st.local?.paused);
  const mode = useStore((st) => st.mode);
  const active = page === "device" ? "ecosystem" : page;
  return (
    <nav className={s.rail} aria-label="Main">
      <div className={s.brand} title="Devti">
        <Logo size={34} />
      </div>
      <ul className={s.items}>
        {ITEMS.map((it) => (
          <li key={it.page}>
            <button
              type="button"
              className={s.item}
              aria-current={active === it.page ? "page" : undefined}
              onClick={() => navigate(it.page)}
              data-tour={`nav-${it.page}`}
            >
              <Icon name={it.icon} size={22} />
              <span>{it.label}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className={s.bottom}>
        {paused && (
          <span className={s.paused} role="status">
            <Icon name="pause" size={14} />
            Paused
          </span>
        )}
        <button type="button" className={s.pair} onClick={onPair} data-tour="pair" disabled={paused}>
          <Icon name="plus" size={22} />
          <span>Pair</span>
        </button>
        {mode === "preview" && (
          <span className={s.preview} title="Running in a browser with simulated devices">
            Preview
          </span>
        )}
      </div>
    </nav>
  );
}
