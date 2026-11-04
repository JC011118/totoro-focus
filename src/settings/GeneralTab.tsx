import { useState } from "react";
import type { AppInfo } from "../ipc/bindings/AppInfo";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { UserSettingsPatch } from "../ipc/bindings/UserSettingsPatch";
import { errorMessage } from "../ipc/errors";
import { useBackend, useSnapshotCommand } from "../state/BackendContext";
import { usePrefersReducedMotion } from "../state/useReducedMotion";
import { HotkeyRecorder } from "./HotkeyRecorder";
import { Button, Message, Row, Section, Toggle } from "./ui";

export function GeneralTab({ snapshot, appInfo }: { snapshot: Snapshot; appInfo: AppInfo | null }) {
  const backend = useBackend();
  const command = useSnapshotCommand();
  const systemReduced = usePrefersReducedMotion();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const s = snapshot.settings;

  const patch = (p: UserSettingsPatch) => {
    setError(null);
    command(backend.updateSettings(p)).catch((e: unknown) => setError(errorMessage(e)));
  };
  const act = (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    fn()
      .catch((e: unknown) => setError(errorMessage(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="st-tab">
      <Section title="Companion">
        <Toggle
          label="Sound"
          hint="A soft chime when a session or a break ends."
          checked={s.sound}
          onChange={(v) => patch({ sound: v })}
        />
        <Toggle
          label="Reduced motion"
          hint={
            systemReduced
              ? "Your Mac's Reduce motion setting is on, so the companion stays still either way."
              : "Keep the companion still. The timer works exactly the same."
          }
          checked={s.reducedMotion}
          onChange={(v) => patch({ reducedMotion: v })}
        />
        <Toggle
          label="Hide companion during focus"
          hint="It steps aside when a session starts and comes back when it completes."
          checked={s.hideDuringFocus}
          onChange={(v) => patch({ hideDuringFocus: v })}
        />
        <Row
          label="Companion window"
          hint={snapshot.companionVisible ? "Showing on your desktop." : "Hidden. The timer keeps running either way."}
        >
          {snapshot.companionVisible ? (
            <Button disabled={busy} onClick={() => act(() => backend.hideCompanion())}>
              Hide
            </Button>
          ) : (
            <Button disabled={busy} onClick={() => act(() => backend.showCompanion())}>
              Show
            </Button>
          )}
        </Row>
      </Section>

      <Section title="Menu bar & keyboard">
        <Toggle
          label="Countdown in the menu bar"
          hint="Shows mm:ss next to the menu-bar icon while a session runs."
          checked={s.menuBarTitle}
          onChange={(v) => patch({ menuBarTitle: v })}
        />
        <HotkeyRecorder hotkey={s.hotkey} registrationError={appInfo?.hotkeyError ?? null} />
      </Section>

      {error ? <Message tone="error">{error}</Message> : null}

      <Section title="About">
        <Row
          label={appInfo ? `${appInfo.name} ${appInfo.version}` : "…"}
          hint="Works offline. Your data stays on this Mac."
        >
          <Button variant="danger" onClick={() => act(() => backend.quitApp())}>
            Quit
          </Button>
        </Row>
        {appInfo ? (
          <Row label="Data folder" hint={<code className="st-path">{appInfo.dataDir}</code>} />
        ) : null}
      </Section>
    </div>
  );
}
