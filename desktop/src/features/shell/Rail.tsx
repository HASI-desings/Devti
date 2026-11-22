import { Icon, type IconName } from "../../design/icons";
import { Logo } from "../../design/Prism";
import { type Page, useStore } from "../../state/store";
import s from "./Rail.module.css";

const ITEMS: { page: Page; label: string; icon: IconName }[] = [
  { page: "ecosystem", label: "Ecosystem", icon: "ecosystem" },
  { page: "files", label: "Files", icon: "folder" },
  { page: "notifications", label: "Alerts", icon: "bell" },
  { page: "messages", label: "Messages", icon: "message" },
  { page: "battery", label: "Battery", icon: "battery" },
  { page: "screens", label: "Screens", icon: "monitor" },
  { page: "accessories", label: "Accessories", icon: "bluetooth" },
  { page: "control", label: "Control", icon: "deck" },
  { page: "automation", label: "Automation", icon: "flow" },
  { page: "plugins", label: "Plugins", icon: "plugin" },
  { page: "security", label: "Security", icon: "shield" },
  { page: "settings", label: "Settings", icon: "settings" },
  { page: "help", label: "Help", icon: "help" },
];

export function Rail({ onPair }: { onPair: () => void }) {
  const page = useStore((st) => st.page);
  const navigate = useStore((st) => st.navigate);
  const paused = useStore((st) => st.local?.paused);
  const mode = useStore((st) => st.mode);
  const unread = useStore((st) => st.notifications.length);
  const active_ = useStore((st) => Object.values(st.transfers).filter((t) => t.state === "active").length);
  const active = page === "device" || page === "mirror" ? "ecosystem" : page;
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
              {it.page === "notifications" && unread > 0 && (
                <span className={s.badge}>
                  {unread > 99 ? "99+" : unread}
                  <span className="visually-hidden"> notifications</span>
                </span>
              )}
              {it.page === "files" && active_ > 0 && (
                <span className={s.activity}>
                  <span className="visually-hidden">Transfers running</span>
                </span>
              )}
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
