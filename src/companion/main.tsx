import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createTauriBackend } from "../ipc/tauriBackend";
import { BackendProvider } from "../state/BackendContext";
import { CompanionApp } from "./CompanionApp";
import "../styles/base.css";
import "./companion.css";
import "./page.css";

// Production entry for the companion panel (companion.html). The Tauri
// backend is imported statically: no runtime switch, no mock.
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <BackendProvider backend={createTauriBackend()}>
        <CompanionApp />
      </BackendProvider>
    </StrictMode>,
  );
}
