import type { CSSProperties } from "react";
import { deviceIcon, Icon } from "../../design/icons";
import { voiceVar } from "../../design/voice";
import { useBridge } from "../../lib/bridge-context";
import { orderedPeers, useStore } from "../../state/store";
import { Button, Sheet } from "../../ui";
import s from "./Files.module.css";

/** Display name of a path (preview paths are "preview:<name>:<size>"). */
export function fileName(path: string): string {
  if (path.startsWith("preview:")) return path.split(":")[1] ?? path;
  return path.split(/[\\/]/).pop() || path;
}

/** "Where should these files go?" — after drag and drop or Send to Devti. */
export function SendPicker() {
  const bridge = useBridge();
  const paths = useStore((st) => st.sendPaths);
  const setPaths = useStore((st) => st.setSendPaths);
  const peers = orderedPeers(useStore((st) => st.peers));
  const voices = useStore((st) => st.voices);
  const toast = useStore((st) => st.toast);
  if (!paths) return null;
  const names = paths.map(fileName);

  const send = async (id: string) => {
    setPaths(null);
    try {
      await bridge.sendFiles(id, paths);
    } catch (e) {
      toast(String(e instanceof Error ? e.message : e), "danger");
    }
  };

  return (
    <Sheet isOpen onClose={() => setPaths(null)} label="Send files" narrow>
      <div className={s.picker}>
        <h2 className={s.pickerTitle}>Send {paths.length === 1 ? names[0] : `${paths.length} files`} to…</h2>
        {peers.length === 0 ? (
          <p className={s.muted}>Pair a device first, then send files to it.</p>
        ) : (
          <ul className={s.pickerList}>
            {peers.map((p) => (
              <li key={p.info.id}>
                <button
                  type="button"
                  className={s.pickerRow}
                  style={{ "--voice": voiceVar(voices[p.info.id] ?? "iris") } as CSSProperties}
                  onClick={() => void send(p.info.id)}
                >
                  <Icon
                    name={deviceIcon(p.info.kind)}
                    size={22}
                    dotColor={p.online ? voiceVar(voices[p.info.id] ?? "iris") : undefined}
                  />
                  <span className={s.pickerName}>{p.info.name}</span>
                  <span className={s.muted}>{p.online ? "Sends now" : "Sends when it's back online"}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <Button variant="ghost" onPress={() => setPaths(null)}>
          Cancel
        </Button>
      </div>
    </Sheet>
  );
}
