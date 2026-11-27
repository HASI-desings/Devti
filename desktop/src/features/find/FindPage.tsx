import "leaflet/dist/leaflet.css";
import L from "leaflet";
import { type CSSProperties, useEffect, useMemo, useRef, useState } from "react";
import type { DeviceKind, DeviceLocationInfo } from "../../bridge";
import { deviceIcon, Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { relativeTime } from "../../lib/format";
import { orderedPeers, useStore } from "../../state/store";
import { Button } from "../../ui";
import p from "../shell/page.module.css";
import s from "./Find.module.css";
import { StolenSheet } from "./StolenSheet";

const err = (e: unknown) => String(e instanceof Error ? e.message : e);
const HERE = "this";

interface Row {
  id: string;
  name: string;
  kind: DeviceKind;
  color: string;
  online: boolean;
  isHere: boolean;
  platform: string;
}

/**
 * Find my devices: every device on one map, with where it was last seen, its network and
 * its IP addresses. Locations travel only between your own devices; the map tiles come
 * from OpenStreetMap.
 */
export function FindPage() {
  const bridge = useBridge();
  const toast = useStore((st) => st.toast);
  const local = useStore((st) => st.local)!;
  const peers = orderedPeers(useStore((st) => st.peers));
  const voices = useStore((st) => st.voices);
  const [locs, setLocs] = useState<Record<string, DeviceLocationInfo>>({});
  const [asking, setAsking] = useState<Record<string, boolean>>({});
  const [focus, setFocus] = useState<string | null>(null);
  const [stolenFor, setStolenFor] = useState<string | null>(null);
  const onStolen = (id: string) => setStolenFor(id);

  useEffect(() => {
    void bridge.deviceLocations().then(
      (list) =>
        setLocs((cur) => ({ ...Object.fromEntries(list.map((d) => [d.device, d.location])), ...cur })),
      () => {},
    );
    return bridge.onEvent((e) => {
      if (e.type === "location_received") {
        setLocs((cur) => ({ ...cur, [e.from]: e.location }));
        setAsking((a) => ({ ...a, [e.from]: false }));
      }
    });
  }, [bridge]);

  const locateHere = async () => {
    setAsking((a) => ({ ...a, [HERE]: true }));
    try {
      const l = await bridge.ownLocation();
      setLocs((cur) => ({ ...cur, [HERE]: l }));
    } catch (e) {
      toast(err(e), "danger");
    } finally {
      setAsking((a) => ({ ...a, [HERE]: false }));
    }
  };

  // This computer's own place, once, on open.
  // biome-ignore lint/correctness/useExhaustiveDependencies: run once on open
  useEffect(() => {
    void locateHere();
  }, []);

  const locate = async (id: string) => {
    setAsking((a) => ({ ...a, [id]: true }));
    try {
      await bridge.locateDevice(id);
      // No answer in 20 s: stop the spinner (the device may not allow it).
      window.setTimeout(() => setAsking((a) => ({ ...a, [id]: false })), 20_000);
    } catch (e) {
      setAsking((a) => ({ ...a, [id]: false }));
      toast(err(e), "danger");
    }
  };

  const rows: Row[] = useMemo(
    () => [
      {
        id: HERE,
        name: local.info.name,
        kind: local.info.kind,
        color: "var(--hub)",
        online: true,
        isHere: true,
        platform: local.info.platform,
      },
      ...peers.map((peer) => ({
        id: peer.info.id,
        name: peer.info.name,
        kind: peer.info.kind,
        color: voiceVar(voices[peer.info.id] ?? "iris"),
        online: peer.online,
        isHere: false,
        platform: peer.info.platform,
      })),
    ],
    [local, peers, voices],
  );

  return (
    <div className={p.page}>
      <div className={s.wrap}>
        <header className={p.hero}>
          <h1 className={p.title}>Find my devices</h1>
          <p className={p.intro}>
            Where each device is, the network it's on and its IP address. Locations go only to your own
            devices, encrypted, and work from anywhere.
          </p>
        </header>
        <div className={s.layout}>
          <DeviceMap rows={rows} locs={locs} focus={focus} />
          <ul className={s.list} aria-label="Devices">
            {rows.map((r) => {
              const l = locs[r.id];
              const pos = l?.lat != null && l?.lon != null;
              return (
                <li key={r.id} className={s.card} style={{ "--voice": r.color } as CSSProperties}>
                  <div className={s.cardHead}>
                    <span className={s.cardIcon} data-online={r.online || undefined}>
                      <Icon name={deviceIcon(r.kind)} size={22} />
                    </span>
                    <div className={p.itemText}>
                      <span className={p.itemTitle}>
                        {r.name}
                        {r.isHere && <span className={s.here}> · this computer</span>}
                      </span>
                      <span className={p.itemHint}>
                        {asking[r.id]
                          ? "Asking where it is…"
                          : !l
                            ? r.online
                              ? "Not located yet."
                              : "Offline. Not located yet."
                            : `${l.live ? "Live" : "Seen"} ${relativeTime(l.ts)}${r.online ? "" : " · offline now"}`}
                      </span>
                    </div>
                  </div>
                  {l && (
                    <dl className={s.facts}>
                      <dt>Place</dt>
                      <dd>
                        {pos ? (
                          <button
                            type="button"
                            className={s.coords}
                            onClick={() => setFocus(`${r.id}:${Date.now()}`)}
                          >
                            <span className="num">
                              {l.lat?.toFixed(5)}, {l.lon?.toFixed(5)}
                            </span>
                            {l.accuracy_m != null && <> ± {Math.round(l.accuracy_m)} m</>}
                          </button>
                        ) : (
                          <span className={s.muted}>{l.error ?? "No position"}</span>
                        )}
                      </dd>
                      {l.network && (
                        <>
                          <dt>Network</dt>
                          <dd>{l.network}</dd>
                        </>
                      )}
                      <dt>Public IP</dt>
                      <dd className={p.mono}>{l.public_ip ?? "unknown"}</dd>
                      {l.local_ips.length > 0 && (
                        <>
                          <dt>Local IP</dt>
                          <dd className={p.mono}>{l.local_ips.slice(0, 3).join(", ")}</dd>
                        </>
                      )}
                      {l.battery != null && (
                        <>
                          <dt>Battery</dt>
                          <dd className="num">{l.battery}%</dd>
                        </>
                      )}
                    </dl>
                  )}
                  <div className={p.row}>
                    {r.isHere ? (
                      <Button
                        size="sm"
                        icon="radar"
                        isDisabled={asking[r.id]}
                        onPress={() => void locateHere()}
                      >
                        Locate
                      </Button>
                    ) : (
                      <>
                        <Button
                          size="sm"
                          icon="radar"
                          isDisabled={!r.online || asking[r.id]}
                          onPress={() => void locate(r.id)}
                        >
                          Locate
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          icon="bell"
                          isDisabled={!r.online}
                          onPress={() =>
                            void bridge
                              .ring(r.id, true)
                              .then(() => toast(`Ringing ${r.name}.`, "info"))
                              .catch((e) => toast(err(e), "danger"))
                          }
                        >
                          Ring
                        </Button>
                      </>
                    )}
                    {pos && (
                      <Button
                        size="sm"
                        variant="ghost"
                        icon="external"
                        onPress={() =>
                          void bridge.openExternal(
                            `https://www.openstreetmap.org/?mlat=${l?.lat}&mlon=${l?.lon}#map=17/${l?.lat}/${l?.lon}`,
                          )
                        }
                      >
                        Open map
                      </Button>
                    )}
                    {!r.isHere && (r.kind === "phone" || r.kind === "tablet") && (
                      <Button size="sm" variant="danger" icon="lock" onPress={() => onStolen(r.id)}>
                        Stolen mode
                      </Button>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
      </div>
      <StolenSheet
        peer={peers.find((x) => x.info.id === stolenFor) ?? null}
        onClose={() => setStolenFor(null)}
      />
    </div>
  );
}

/** The map: one dot per device in its voice, with its accuracy circle. */
function DeviceMap({
  rows,
  locs,
  focus,
}: {
  rows: Row[];
  locs: Record<string, DeviceLocationInfo>;
  focus: string | null;
}) {
  const el = useRef<HTMLElement>(null);
  const map = useRef<L.Map | null>(null);
  const layer = useRef<L.LayerGroup | null>(null);
  const fitted = useRef(false);
  const [tilesFailed, setTilesFailed] = useState(false);

  useEffect(() => {
    if (!el.current || map.current) return;
    const m = L.map(el.current, { zoomControl: true, attributionControl: true, worldCopyJump: true }).setView(
      [30.3753, 69.3451],
      4,
    );
    m.attributionControl.setPrefix('<a href="https://leafletjs.com">Leaflet</a>');
    const tiles = L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
      maxZoom: 19,
      attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
    }).addTo(m);
    let failures = 0;
    tiles.on("tileerror", () => {
      failures += 1;
      if (failures >= 4) setTilesFailed(true);
    });
    tiles.on("tileload", () => setTilesFailed(false));
    layer.current = L.layerGroup().addTo(m);
    map.current = m;
    // The container may still be sizing itself on first paint.
    const t = window.setTimeout(() => m.invalidateSize(), 50);
    return () => {
      window.clearTimeout(t);
      m.remove();
      map.current = null;
    };
  }, []);

  useEffect(() => {
    const m = map.current;
    const g = layer.current;
    if (!m || !g) return;
    g.clearLayers();
    const points: L.LatLngExpression[] = [];
    for (const r of rows) {
      const l = locs[r.id];
      if (l?.lat == null || l?.lon == null) continue;
      const at: L.LatLngExpression = [l.lat, l.lon];
      points.push(at);
      const color = getComputedStyle(el.current ?? document.body)
        .getPropertyValue(r.color.replace(/^var\(|\)$/g, ""))
        .trim();
      if (l.accuracy_m) {
        L.circle(at, {
          radius: l.accuracy_m,
          color: color || "#888",
          weight: 1,
          fillOpacity: 0.12,
        }).addTo(g);
      }
      L.marker(at, {
        icon: L.divIcon({
          className: s.pin,
          html: `<span style="--pin:${color || "#888"}" data-live="${l.live}"></span>`,
          iconSize: [22, 22],
          iconAnchor: [11, 11],
        }),
        title: r.name,
        alt: r.name,
      })
        .bindTooltip(r.name, { direction: "top", offset: [0, -12] })
        .addTo(g);
    }
    if (points.length && !fitted.current) {
      fitted.current = true;
      m.fitBounds(L.latLngBounds(points).pad(0.4), { maxZoom: 16 });
    }
  }, [rows, locs]);

  useEffect(() => {
    if (!focus || !map.current) return;
    const id = focus.split(":")[0]!;
    const l = locs[id];
    if (l?.lat != null && l?.lon != null) map.current.flyTo([l.lat, l.lon], 17, { duration: 0.6 });
  }, [focus, locs]);

  return (
    <div className={s.mapWrap}>
      <section ref={el} className={s.map} aria-label="Map of your devices" data-testid="find-map" />
      {tilesFailed && (
        <p className={s.offline}>
          The map needs the internet to draw streets. Devices and their coordinates still show.
        </p>
      )}
    </div>
  );
}
