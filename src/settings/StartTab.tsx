// Settings → Start: the place for typing (task label, exact minutes), since
// the companion panel has no inputs. Starting here is the same command the
// widget, tray and hotkey send.

import { useState, type FormEvent } from "react";
import type { Limits } from "../ipc/bindings/Limits";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { errorMessage } from "../ipc/errors";
import { clampMinutes, parseMinutes, stepMinutes } from "../format/minutes";
import { useBackend, useSnapshotCommand } from "../state/BackendContext";
import { Button, Message, Section } from "./ui";

export function StartTab({ snapshot, limits }: { snapshot: Snapshot; limits: Limits }) {
  const backend = useBackend();
  const command = useSnapshotCommand();
  const [label, setLabel] = useState("");
  const [minutesText, setMinutesText] = useState(() => String(snapshot.settings.lastFocusMinutes));
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "error" | "ok"; text: string } | null>(null);

  const minutes = parseMinutes(minutesText, limits);
  const session = snapshot.session;
  const canStart = snapshot.phase === "idle" || snapshot.phase === "awaitingChoice";

  const run = async (fn: () => Promise<unknown>, ok?: string) => {
    setBusy(true);
    setMessage(null);
    try {
      await fn();
      if (ok) setMessage({ tone: "ok", text: ok });
    } catch (e) {
      setMessage({ tone: "error", text: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  const startFocus = (e?: FormEvent) => {
    e?.preventDefault();
    if (!canStart || busy) return;
    if (minutes === null) {
      setMessage({ tone: "error", text: `Choose between ${limits.minMinutes} and ${limits.maxMinutes} minutes.` });
      return;
    }
    const taskLabel = label.trim() || undefined;
    void run(async () => {
      await command(backend.startFocus({ minutes, taskLabel }));
      setLabel("");
    }, `Focus started: ${minutes} min.`);
  };

  const step = (dir: 1 | -1) =>
    setMinutesText((t) => String(stepMinutes(parseMinutes(t, limits) ?? clampMinutes(Number(t) || 25, limits), dir, limits)));

  return (
    <div className="st-tab">
      {session ? <CurrentSession snapshot={snapshot} busy={busy} run={run} /> : null}

      <Section title="Start a focus session">
        <form className="st-start" onSubmit={startFocus}>
          <label className="st-field">
            <span className="st-field__label">Task (optional)</span>
            <input
              className="st-input"
              type="text"
              value={label}
              maxLength={limits.maxTaskLabelChars}
              placeholder="e.g. Review one lecture"
              autoComplete="off"
              spellCheck
              onChange={(e) => setLabel(e.target.value)}
            />
            <span className="st-field__hint">
              Shown on the companion while you focus. {label.length > 0 ? `${label.length}/${limits.maxTaskLabelChars}` : ""}
            </span>
          </label>

          <div className="st-field">
            <span className="st-field__label" id="st-minutes-label">
              Length
            </span>
            <div className="st-minutes">
              <button
                type="button"
                className="st-step"
                aria-label="Fewer minutes"
                onClick={() => step(-1)}
                disabled={minutes !== null && minutes <= limits.minMinutes}
              >
                −
              </button>
              <input
                className="st-input st-input--minutes"
                type="number"
                inputMode="numeric"
                min={limits.minMinutes}
                max={limits.maxMinutes}
                step={1}
                value={minutesText}
                aria-labelledby="st-minutes-label"
                aria-invalid={minutes === null}
                onChange={(e) => setMinutesText(e.target.value)}
              />
              <button
                type="button"
                className="st-step"
                aria-label="More minutes"
                onClick={() => step(1)}
                disabled={minutes !== null && minutes >= limits.maxMinutes}
              >
                +
              </button>
              <span className="st-minutes__unit">min</span>
              <div className="st-presets" role="group" aria-label="Presets">
                {limits.presets.map((p) => (
                  <button
                    key={p}
                    type="button"
                    className={`st-chip${minutes === p ? " st-chip--on" : ""}`}
                    onClick={() => setMinutesText(String(p))}
                  >
                    {p}
                  </button>
                ))}
              </div>
            </div>
            <span className="st-field__hint">
              {limits.minMinutes}–{limits.maxMinutes} minutes.
            </span>
          </div>

          <div className="st-actions">
            <Button type="submit" variant="primary" icon="play" disabled={!canStart || busy || minutes === null}>
              Start focus{minutes !== null ? ` · ${minutes} min` : ""}
            </Button>
            <Button
              icon="cup"
              disabled={!canStart || busy}
              onClick={() =>
                void run(() => command(backend.startBreak()), `Break started: ${limits.breakMinutes} min.`)
              }
            >
              Start a {limits.breakMinutes}-min break
            </Button>
          </div>
          {!canStart ? <Message>A session is going. End it to start a new one.</Message> : null}
          {message ? <Message tone={message.tone}>{message.text}</Message> : null}
        </form>
      </Section>
    </div>
  );
}

function CurrentSession({
  snapshot,
  busy,
  run,
}: {
  snapshot: Snapshot;
  busy: boolean;
  run: (fn: () => Promise<unknown>) => Promise<void>;
}) {
  const backend = useBackend();
  const command = useSnapshotCommand();
  const s = snapshot.session;
  if (!s) return null;
  const paused = s.status === "paused";
  return (
    <Section title="Now">
      <div className="st-now">
        <div className="st-now__time" role="timer">
          {s.remainingLabel}
        </div>
        <div className="st-now__text">
          <div className="st-now__title">
            {s.kind === "break" ? "Break" : "Focus"}
            {paused ? <span className="st-badge">paused</span> : null}
          </div>
          <div className="st-now__sub">{s.taskLabel ?? (s.kind === "break" ? "Resting" : "No task label")}</div>
        </div>
        <div className="st-now__actions">
          {paused ? (
            <Button icon="play" variant="primary" disabled={busy} onClick={() => void run(() => command(backend.resume()))}>
              Resume
            </Button>
          ) : (
            <Button icon="pause" disabled={busy} onClick={() => void run(() => command(backend.pause()))}>
              Pause
            </Button>
          )}
          <Button icon="stop" disabled={busy} onClick={() => void run(() => command(backend.endSession()))}>
            End
          </Button>
        </div>
      </div>
    </Section>
  );
}
