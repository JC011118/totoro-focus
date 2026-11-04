// Records a global shortcut. While recording, the shell unregisters the live
// binding (set_hotkey_capture), so pressing the current shortcut is recorded
// instead of toggling the timer. Keys come from KeyboardEvent.code.

import { useEffect, useEffectEvent, useRef, useState } from "react";
import { errorMessage } from "../ipc/errors";
import { DEFAULT_HOTKEY, displayAccelerator, recordAccelerator } from "../format/hotkey";
import { useBackend, useSnapshotCommand } from "../state/BackendContext";
import { Button, Message } from "./ui";

/** The shell restores the binding after 30 s of capture; stop recording at the same time. */
export const RECORD_TIMEOUT_MS = 30_000;

type Mode = { kind: "idle" } | { kind: "recording"; held: string } | { kind: "saving" };

export function HotkeyRecorder({ hotkey, registrationError }: { hotkey: string | null; registrationError: string | null }) {
  const backend = useBackend();
  const command = useSnapshotCommand();
  const [mode, setMode] = useState<Mode>({ kind: "idle" });
  const [error, setError] = useState<string | null>(null);
  const capturing = useRef(false);

  const releaseCapture = () => {
    if (!capturing.current) return;
    capturing.current = false;
    backend.setHotkeyCapture(false).catch(() => {});
  };

  const start = async () => {
    setError(null);
    try {
      await backend.setHotkeyCapture(true);
      capturing.current = true;
      setMode({ kind: "recording", held: "" });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const cancel = () => {
    releaseCapture();
    setMode({ kind: "idle" });
  };

  const save = async (accelerator: string | null) => {
    setMode({ kind: "saving" });
    setError(null);
    try {
      await command(backend.setHotkey(accelerator));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      releaseCapture();
      setMode({ kind: "idle" });
    }
  };

  const onKey = useEffectEvent((e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    const r = recordAccelerator(e);
    switch (r.kind) {
      case "pending":
        setMode({ kind: "recording", held: r.display });
        return;
      case "cancel":
        cancel();
        return;
      case "clear":
        void save(null);
        return;
      case "invalid":
        setError(r.reason);
        return;
      case "accelerator":
        void save(r.accelerator);
        return;
    }
  });
  const onKeyUp = useEffectEvent((e: KeyboardEvent) => {
    const r = recordAccelerator(e);
    if (r.kind === "pending") setMode({ kind: "recording", held: r.display });
  });
  const stop = useEffectEvent(() => cancel());

  const recording = mode.kind === "recording";
  useEffect(() => {
    if (!recording) return;
    const down = (e: KeyboardEvent) => onKey(e);
    const up = (e: KeyboardEvent) => onKeyUp(e);
    const blur = () => stop();
    const timer = setTimeout(blur, RECORD_TIMEOUT_MS);
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("blur", blur);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("blur", blur);
    };
  }, [recording]);

  // Never leave the shortcut unregistered if the tab goes away mid-recording.
  const releaseOnUnmount = useEffectEvent(releaseCapture);
  useEffect(() => () => releaseOnUnmount(), []);

  const current = displayAccelerator(hotkey);
  return (
    <div className="st-hotkey">
      <div className="st-row">
        <div className="st-row__text">
          <div className="st-row__label">Start / pause shortcut</div>
          <div className="st-row__hint">
            {recording
              ? "Press the new shortcut. Esc cancels, Delete turns it off."
              : "Idle: start your last length. Running: pause. Paused: resume."}
          </div>
        </div>
        <div className="st-row__control st-hotkey__control">
          <kbd className={`st-kbd${recording ? " st-kbd--recording" : ""}${!hotkey && !recording ? " st-kbd--off" : ""}`}>
            {recording ? mode.held || "Type shortcut…" : current || "Off"}
          </kbd>
          {recording ? (
            <Button onClick={cancel}>Cancel</Button>
          ) : (
            <Button disabled={mode.kind === "saving"} onClick={() => void start()}>
              {hotkey ? "Change…" : "Set…"}
            </Button>
          )}
        </div>
      </div>
      <div className="st-hotkey__extra">
        {hotkey && !recording ? (
          <Button variant="quiet" disabled={mode.kind === "saving"} onClick={() => void save(null)}>
            Disable shortcut
          </Button>
        ) : null}
        {hotkey !== DEFAULT_HOTKEY && !recording ? (
          <Button variant="quiet" disabled={mode.kind === "saving"} onClick={() => void save(DEFAULT_HOTKEY)}>
            Use {displayAccelerator(DEFAULT_HOTKEY)}
          </Button>
        ) : null}
      </div>
      {error ? <Message tone="error">{error}</Message> : null}
      {registrationError && !error ? (
        <Message tone="error">The saved shortcut couldn't be registered: {registrationError}</Message>
      ) : null}
    </div>
  );
}
