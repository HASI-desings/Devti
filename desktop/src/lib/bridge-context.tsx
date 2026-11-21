import { createContext, type ReactNode, useContext } from "react";
import type { Bridge } from "../bridge";

const Ctx = createContext<Bridge | null>(null);

export function BridgeProvider({ bridge, children }: { bridge: Bridge; children: ReactNode }) {
  return <Ctx.Provider value={bridge}>{children}</Ctx.Provider>;
}

export function useBridge(): Bridge {
  const b = useContext(Ctx);
  if (!b) throw new Error("useBridge must be used inside <BridgeProvider>");
  return b;
}
