import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createTauriBackend } from "../ipc/tauriBackend";
import { BackendProvider } from "../state/BackendContext";
import { SettingsApp } from "./SettingsApp";
import "../styles/base.css";
import "./settings.css";
import "./page.css";

// Production entry for the settings window (settings.html#<tab>). The Tauri
// backend is imported statically: no runtime switch, no mock.
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <BackendProvider backend={createTauriBackend()}>
        <SettingsApp />
      </BackendProvider>
    </StrictMode>,
  );
}
