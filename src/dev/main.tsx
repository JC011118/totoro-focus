// Dev preview (dev.html, `npm run dev` → http://127.0.0.1:1420/dev.html).
// Renders the companion in a 240×200 frame next to the settings app, both over
// ONE in-memory MockBackend, plus a toolbar to jump between states. Never built
// for production (not a Vite input; scripts/assert-no-mock.mjs enforces it).
//
// Query params (for screenshots): view=companion|settings, preset=<DevPreset>,
// rm=1 (reduced motion), pack=missing-art|fixtures|placeholder, palette=<id>,
// tab=<settings tab>, menu=1 (open the character menu), picker=1.

import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { CompanionApp } from "../companion/CompanionApp";
import type { SettingsTab } from "../ipc/bindings/SettingsTab";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { SettingsApp } from "../settings/SettingsApp";
import { tabFromHash } from "../settings/tabs";
import { BackendProvider } from "../state/BackendContext";
import { loadRepoPlaceholder } from "./devPacks";
import { DevToolbar } from "./DevToolbar";
import { MockBackend, __FOCUS_DEV_MOCK__, type DevPreset } from "./mockBackend";
import "../styles/base.css";
import "../companion/companion.css";
import "../settings/settings.css";
import "./dev.css";

const params = new URLSearchParams(window.location.search);
const view = params.get("view");
const mock = new MockBackend();
document.documentElement.dataset.mock = __FOCUS_DEV_MOCK__;

const preset = params.get("preset") as DevPreset | null;
if (preset) mock.preset(preset);
if (params.get("rm") === "1") mock.setReducedMotion(true);
const palette = params.get("palette");
if (palette) mock.setPalette(palette);
const packParam = params.get("pack");
if (packParam && packParam !== "placeholder") void mock.setActivePack(packParam);

// Use the repo's real placeholder pack when it exists (after the art lands).
void loadRepoPlaceholder().then((p) => {
  if (!p) return;
  mock.addPack(p, packParam === "placeholder" || packParam === null);
});

// menu=1 / picker=1: open the character menu or the stepper once, after mount.
let openedFromParams = false;
function useParamOpeners() {
  useEffect(() => {
    if (openedFromParams) return;
    openedFromParams = true;
    setTimeout(() => {
      if (params.get("menu") === "1") {
        (document.querySelector('[aria-label="Companion menu"]') as HTMLButtonElement | null)?.click();
      }
      if (params.get("picker") === "1") mock.openPicker();
    }, 250);
  }, []);
}

function CompanionFrame() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  useEffect(() => {
    let un: (() => void) | undefined;
    void mock.on("snapshot", setSnap).then((u) => (un = u));
    void mock.getSnapshot().then(setSnap);
    return () => un?.();
  }, []);
  useParamOpeners();
  const hidden = snap ? !snap.companionVisible : false;
  return (
    <div className="dev-frame-wrap">
      <div className={`dev-frame${hidden ? " dev-frame--hidden" : ""}`}>
        <BackendProvider backend={mock}>
          <CompanionApp />
        </BackendProvider>
      </div>
      {hidden ? (
        <button type="button" className="dev-btn dev-frame__show" onClick={() => void mock.showCompanion()}>
          Companion hidden · show
        </button>
      ) : null}
    </div>
  );
}

function Harness() {
  const tab: SettingsTab = tabFromHash(`#${params.get("tab") ?? window.location.hash}`);
  return (
    <div className="dev">
      <DevToolbar mock={mock} />
      <div className="dev-stage">
        <section className="dev-desk" aria-label="Desktop">
          <CompanionFrame />
          <p className="dev-caption">companion.html · 240×200 transparent panel</p>
        </section>
        <section className="dev-window" aria-label="Settings window">
          <div className="dev-window__bar">
            <span />
            <span />
            <span />
            <em>settings.html</em>
          </div>
          <div className="dev-window__body">
            <BackendProvider backend={mock}>
              <SettingsApp initialTab={tab} syncHash={false} />
            </BackendProvider>
          </div>
        </section>
      </div>
    </div>
  );
}

function Solo() {
  useParamOpeners();
  if (view === "companion") {
    return (
      <div className="dev-solo dev-solo--companion">
        <BackendProvider backend={mock}>
          <CompanionApp />
        </BackendProvider>
      </div>
    );
  }
  const tab: SettingsTab = tabFromHash(`#${params.get("tab") ?? ""}`);
  return (
    <div className="dev-solo dev-solo--settings">
      <BackendProvider backend={mock}>
        <SettingsApp initialTab={tab} syncHash={false} />
      </BackendProvider>
    </div>
  );
}

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(<StrictMode>{view ? <Solo /> : <Harness />}</StrictMode>);
}
