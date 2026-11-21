import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { createBridge } from "./bridge";
import { BridgeProvider } from "./lib/bridge-context";
import "./styles/base.css";

const bridge = createBridge();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BridgeProvider bridge={bridge}>
      <App />
    </BridgeProvider>
  </StrictMode>,
);
