// The settings window: tabs Start, General, History, Companion. The initial
// tab comes from location.hash (settings.html#<tab>); focus://open-tab switches
// tabs when the window already exists (SPEC §8.2).

import { useState } from "react";
import type { SettingsTab } from "../ipc/bindings/SettingsTab";
import { DEFAULT_LIMITS } from "../format/minutes";
import { useActivePack, useAppInfo, useBackendEvent, useSnapshot } from "../state/BackendContext";
import { useEffectiveReducedMotion } from "../state/useReducedMotion";
import { useTheme } from "../theme/useTheme";
import { Glyph, type GlyphName } from "../ui/glyphs";
import { CompanionTab } from "./CompanionTab";
import { GeneralTab } from "./GeneralTab";
import { HistoryTab } from "./HistoryTab";
import { StartTab } from "./StartTab";
import { TABS, tabFromHash } from "./tabs";

const TAB_ICON: Record<SettingsTab, GlyphName> = {
  start: "play",
  general: "sliders",
  history: "clock",
  companion: "leaf",
};

export function SettingsApp({ initialTab, syncHash = true }: { initialTab?: SettingsTab; syncHash?: boolean }) {
  const snapshot = useSnapshot();
  const appInfo = useAppInfo();
  const pack = useActivePack();
  const theme = useTheme(snapshot, appInfo, pack);
  const reducedMotion = useEffectiveReducedMotion(snapshot?.settings.reducedMotion);
  const [tab, setTabState] = useState<SettingsTab>(() => initialTab ?? tabFromHash(window.location.hash));

  const setTab = (t: SettingsTab) => {
    setTabState(t);
    if (syncHash) {
      try {
        window.history.replaceState(null, "", `#${t}`);
      } catch {
        // Some embedded contexts disallow history changes; the tab still switches.
      }
    }
  };
  useBackendEvent("openTab", (e) => setTab(e.tab));

  const limits = appInfo?.limits ?? DEFAULT_LIMITS;
  return (
    <div className={`settings${reducedMotion ? " reduced-motion" : ""}`} style={theme.style} data-palette={theme.paletteId}>
      <header className="st-toolbar">
        <div className="st-title">{appInfo?.name ?? "Settings"}</div>
        <nav className="st-tabs" role="tablist" aria-label="Settings sections">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              id={`tab-${t.id}`}
              aria-selected={tab === t.id}
              aria-controls={`panel-${t.id}`}
              className={`st-tabbtn${tab === t.id ? " st-tabbtn--on" : ""}`}
              onClick={() => setTab(t.id)}
            >
              <Glyph name={TAB_ICON[t.id]} size={14} />
              <span>{t.label}</span>
            </button>
          ))}
        </nav>
      </header>
      <main className="st-main" role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`}>
        {!snapshot ? (
          <p className="st-empty">Connecting…</p>
        ) : tab === "start" ? (
          <StartTab snapshot={snapshot} limits={limits} />
        ) : tab === "general" ? (
          <GeneralTab snapshot={snapshot} appInfo={appInfo} />
        ) : tab === "history" ? (
          <HistoryTab snapshot={snapshot} />
        ) : (
          <CompanionTab snapshot={snapshot} appInfo={appInfo} pack={pack} />
        )}
      </main>
    </div>
  );
}
