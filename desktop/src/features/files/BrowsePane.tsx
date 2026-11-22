import type { CSSProperties } from "react";
import { useCallback, useEffect, useState } from "react";
import type { SharedFolder } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { formatBytes } from "../../lib/bytes";
import { relativeTime } from "../../lib/format";
import { useStore } from "../../state/store";
import { Button } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Files.module.css";

const errText = (e: unknown) => String(e instanceof Error ? e.message : e);

/** Files → Browse (Phase 7): what this computer shares, and other devices' shared folders. */
export function BrowsePane() {
  return (
    <>
      <SharedHere />
      <BrowseDevice />
    </>
  );
}

function SharedHere() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const [folders, setFolders] = useState<SharedFolder[]>([]);

  useEffect(() => {
    void bridge.sharedFolders().then(setFolders);
  }, [bridge]);

  const save = async (next: SharedFolder[]) => {
    try {
      await bridge.setSharedFolders(next);
      setFolders(next);
    } catch (e) {
      toast(errText(e), "danger");
    }
  };

  const add = async () => {
    const path = await bridge.pickFolder("Choose a folder to share with your devices");
    if (!path) return;
    const base = path.split(/[\\/]/).filter(Boolean).pop() ?? "Shared";
    let name = base;
    for (let i = 2; folders.some((f) => f.name.toLowerCase() === name.toLowerCase()); i++)
      name = `${base} ${i}`;
    await save([...folders, { name, path }]);
  };

  return (
    <section className={p.section} aria-labelledby="shared-here">
      <div className={s.head}>
        <h2 id="shared-here" className={p.sectionTitle}>
          Shared from this computer
        </h2>
        <Button variant="quiet" size="sm" icon="plus" onPress={() => void add()}>
          Share a folder
        </Button>
      </div>
      {folders.length === 0 ? (
        <p className={s.muted}>
          Nothing is shared. Share a folder and your devices can browse it and download from it — never change
          it.
        </p>
      ) : (
        <ul className={s.transfers}>
          {folders.map((f) => (
            <li key={f.name} className={s.shared}>
              <Icon name="folder" size={22} />
              <div className={s.tMain}>
                <span className={s.tName}>{f.name}</span>
                <span className={`${s.muted} ${p.mono}`}>{f.path}</span>
              </div>
              <Button
                variant="ghost"
                size="sm"
                icon="close"
                iconOnly
                onPress={() => void save(folders.filter((x) => x.name !== f.name))}
              >
                Stop sharing {f.name}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <p className={s.muted}>
        A device also needs “Browse shared folders” in its permissions — on for trusted devices, off for
        restricted ones. Hidden files are never listed.
      </p>
    </section>
  );
}

function BrowseDevice() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const voices = useStore((st) => st.voices);
  const peers = useStore((st) => st.peers);
  const listings = useStore((st) => st.fsListings);
  const online = Object.values(peers).filter((x) => x.online);
  const [device, setDevice] = useState<string | null>(null);
  const [path, setPath] = useState("/");
  const [asked, setAsked] = useState(0);
  const current = device ?? online[0]?.info.id ?? null;
  const listing = current ? listings[`${current}|${path}`] : undefined;
  const loading = !!current && (!listing || listing.at < asked);

  const open = useCallback(
    async (id: string, to: string) => {
      setDevice(id);
      setPath(to);
      setAsked(Date.now());
      try {
        await bridge.fsList(id, to);
      } catch (e) {
        toast(errText(e), "danger");
      }
    },
    [bridge, toast],
  );

  // Only when the device changes; navigation calls open() itself.
  // biome-ignore lint/correctness/useExhaustiveDependencies: listing and path are read once per device
  useEffect(() => {
    if (current && !listing) void open(current, path);
  }, [current, open]);

  const download = async (name: string) => {
    if (!current) return;
    try {
      await bridge.fsGet(current, `${path.replace(/\/$/, "")}/${name}`);
      toast(`Downloading ${name} — it appears under Transfers.`, "info");
    } catch (e) {
      toast(errText(e), "danger");
    }
  };

  const crumbs = path.split("/").filter(Boolean);

  return (
    <section className={p.section} aria-labelledby="browse-device">
      <h2 id="browse-device" className={p.sectionTitle}>
        Browse a device
      </h2>
      {online.length === 0 ? (
        <p className={s.muted}>No device is connected right now.</p>
      ) : (
        <>
          <div className={s.devices} role="tablist" aria-label="Device">
            {online.map((x) => (
              <button
                key={x.info.id}
                type="button"
                role="tab"
                aria-selected={x.info.id === current}
                className={s.deviceChip}
                style={{ "--voice": voiceVar(voices[x.info.id] ?? "iris") } as CSSProperties}
                onClick={() => void open(x.info.id, "/")}
              >
                <Icon
                  name={deviceIcon(x.info.kind)}
                  size={18}
                  dotColor={voiceVar(voices[x.info.id] ?? "iris")}
                />
                {x.info.name}
              </button>
            ))}
          </div>
          <nav className={s.crumbs} aria-label="Folder">
            <button type="button" onClick={() => current && void open(current, "/")}>
              Shared folders
            </button>
            {crumbs.map((c, i) => (
              <span key={crumbs.slice(0, i + 1).join("/")} className={s.crumb}>
                <Icon name="chevronRight" size={14} />
                <button
                  type="button"
                  aria-current={i === crumbs.length - 1 ? "page" : undefined}
                  onClick={() => current && void open(current, `/${crumbs.slice(0, i + 1).join("/")}`)}
                >
                  {c}
                </button>
              </span>
            ))}
          </nav>
          {loading ? (
            <p className={s.muted} aria-live="polite">
              Asking {peers[current ?? ""]?.info.name}…
            </p>
          ) : listing?.error ? (
            <p className={s.muted} role="alert">
              {listing.error}
            </p>
          ) : listing && listing.entries.length === 0 ? (
            <p className={s.muted}>
              {path === "/" ? "This device doesn't share any folders." : "This folder is empty."}
            </p>
          ) : (
            <ul className={s.entries} aria-label={`Contents of ${crumbs.at(-1) ?? "shared folders"}`}>
              {listing?.entries.map((e) => (
                <li key={e.name} className={s.entry}>
                  <Icon name={e.dir ? "folder" : "copy"} size={20} />
                  {e.dir ? (
                    <button
                      type="button"
                      className={s.entryName}
                      onClick={() => current && void open(current, `${path.replace(/\/$/, "")}/${e.name}`)}
                    >
                      {e.name}
                    </button>
                  ) : (
                    <span className={s.entryName}>{e.name}</span>
                  )}
                  <span className={s.muted}>{e.dir ? "" : formatBytes(e.size)}</span>
                  <span className={s.muted}>{e.modified ? relativeTime(e.modified) : ""}</span>
                  {e.dir ? (
                    <span />
                  ) : (
                    <Button
                      variant="ghost"
                      size="sm"
                      icon="download"
                      iconOnly
                      onPress={() => void download(e.name)}
                    >
                      Download {e.name}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
          {listing?.truncated && <p className={s.muted}>Showing the first 2,000 items.</p>}
        </>
      )}
    </section>
  );
}
