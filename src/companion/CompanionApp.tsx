// The 240×200 companion widget. Renders Snapshots and sends commands; it never
// owns timing. Controls are tabIndex=-1 buttons only (no inputs, SPEC §8.1).

import { useEffect, useState, type ReactNode } from "react";
import { CharacterView } from "../character/CharacterView";
import { baseKeyFor } from "../character/mapping";
import { HarvestRow } from "../harvest/HarvestRow";
import type { AwaitingView } from "../ipc/bindings/AwaitingView";
import type { Limits } from "../ipc/bindings/Limits";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { UnlockDef } from "../ipc/bindings/UnlockDef";
import { errorMessage } from "../ipc/errors";
import { DEFAULT_LIMITS, clampMinutes } from "../format/minutes";
import {
  useActivePack,
  useAppInfo,
  useBackend,
  useBackendEvent,
  useSnapshot,
  useSnapshotCommand,
} from "../state/BackendContext";
import { useEffectiveReducedMotion } from "../state/useReducedMotion";
import { useTheme } from "../theme/useTheme";
import { TimerView } from "../timer/TimerView";
import { CharacterMenu, replayItems } from "./CharacterMenu";
import { pauseLine, phaseLabel, unlockLabel, wholeMinutes } from "./copy";
import { DurationStepper } from "./DurationStepper";
import { Header } from "./Header";
import { useCompanionMotion } from "./useCompanionMotion";
import { WidgetButton } from "./WidgetButton";

export const RING_SIZE = 100;
export const CHARACTER_SIZE = 74;
const NOTE_MS = 8000;
const TOAST_MS = 3500;

interface Note {
  text: string;
  nonce: number;
}

/** A message that clears itself after `ms`. */
function useTransient(ms: number): [string | null, (text: string) => void, () => void] {
  const [note, setNote] = useState<Note | null>(null);
  useEffect(() => {
    if (!note) return;
    const t = setTimeout(() => setNote(null), ms);
    return () => clearTimeout(t);
  }, [note, ms]);
  return [note?.text ?? null, (text: string) => setNote((n) => ({ text, nonce: (n?.nonce ?? 0) + 1 })), () => setNote(null)];
}

export function CompanionApp() {
  const backend = useBackend();
  const snapshot = useSnapshot();
  const pack = useActivePack();
  const appInfo = useAppInfo();
  const command = useSnapshotCommand();
  const theme = useTheme(snapshot, appInfo, pack);
  const reducedMotion = useEffectiveReducedMotion(snapshot?.settings.reducedMotion);
  const limits = appInfo?.limits ?? DEFAULT_LIMITS;
  const unlocks = appInfo?.unlocks ?? [];
  const unlocked = snapshot?.friendship.unlocked ?? [];

  const motion = useCompanionMotion({ pack, unlocked, reducedMotion });
  const [menuOpen, setMenuOpen] = useState(false);
  const [picker, setPicker] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, showNote, clearNote] = useTransient(NOTE_MS);
  const [toast, showToast] = useTransient(TOAST_MS);

  const phase = snapshot?.phase ?? "idle";
  const session = snapshot?.session ?? null;
  const lastMinutes = clampMinutes(snapshot?.settings.lastFocusMinutes ?? 25, limits);
  const canStart = snapshot !== null && (phase === "idle" || phase === "awaitingChoice");
  const pickerOpen = picker !== null && canStart;

  const openPicker = () => setPicker(lastMinutes);
  useBackendEvent("openPicker", () => {
    setMenuOpen(false);
    openPicker();
  });
  useBackendEvent("transition", (t) => {
    if (t.type === "focusInterrupted") {
      const min = wholeMinutes(t.record.activeMs);
      const pts = t.award ? t.award.points + t.award.bonus : 0;
      showNote(`Saved ${min} min${pts > 0 ? ` · +${pts}` : ""}`);
    } else if (t.type === "focusStarted" || t.type === "breakStarted") {
      clearNote();
    }
  });

  /** Sends one intent at a time; the reply Snapshot is applied in seq order. */
  const run = (call: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    call()
      .catch((e: unknown) => showToast(errorMessage(e)))
      .finally(() => setBusy(false));
  };
  const startFocus = (minutes: number) =>
    run(async () => {
      await command(backend.startFocus({ minutes }));
      setPicker(null);
    });
  const startBreak = () => run(() => command(backend.startBreak()));

  const baseKey = snapshot ? baseKeyFor(snapshot, theme.idleVariant) : "idle";
  const focusRunning = phase === "focus" && session?.status === "running";
  const awaiting = phase === "awaitingChoice" ? snapshot?.awaiting ?? null : null;

  let heading: ReactNode = null;
  let aside: ReactNode = null;
  if (!snapshot) {
    heading = <Heading title="Hello" sub="Waking up…" />;
  } else if (pickerOpen) {
    heading = <Heading title="Custom" />;
    aside = (
      <DurationStepper
        value={picker ?? lastMinutes}
        limits={limits}
        disabled={busy}
        onChange={(update) => setPicker((v) => update(v ?? lastMinutes))}
        onStart={() => startFocus(picker ?? lastMinutes)}
        onBack={() => setPicker(null)}
      />
    );
  } else if (session) {
    aside = (
      <SessionControls
        snapshot={snapshot}
        busy={busy}
        onPause={() => run(() => command(backend.pause()))}
        onResume={() => run(() => command(backend.resume()))}
        onEnd={() => run(() => command(backend.endSession()))}
      />
    );
  } else if (awaiting && awaiting.after === "focus") {
    heading = <AwaitingHeading awaiting={awaiting} unlocks={unlocks} />;
    aside = (
      <div className="cw-actions">
        <WidgetButton variant="primary" icon="cup" wide disabled={busy} onPress={startBreak}>
          {limits.breakMinutes}-min break
        </WidgetButton>
        <div className="cw-row cw-row--compact">
          <WidgetButton variant="soft" disabled={busy} onPress={() => startFocus(lastMinutes)}>
            Another {lastMinutes}
          </WidgetButton>
          <WidgetButton variant="outline" disabled={busy} onPress={() => run(() => command(backend.dismiss()))}>
            Done
          </WidgetButton>
        </div>
      </div>
    );
  } else {
    const afterBreak = awaiting?.after === "break";
    heading = afterBreak ? (
      <Heading title="Break's over" sub="Another round?" />
    ) : (
      <Heading
        title="Ready"
        sub={note ?? (snapshot.today.focusMs > 0 ? `${snapshot.today.focusLabel} today` : "Whenever you are")}
        accent={note !== null}
      />
    );
    aside = (
      <StartControls
        limits={limits}
        lastMinutes={lastMinutes}
        busy={busy}
        onStart={startFocus}
        onCustom={openPicker}
        onTask={() => run(() => backend.openSettings("start"))}
        onDone={afterBreak ? () => run(() => command(backend.dismiss())) : undefined}
      />
    );
  }

  const next = snapshot?.friendship.next ?? null;
  return (
    <div
      className={`companion companion--${phase}${session?.status === "paused" ? " companion--paused" : ""}${reducedMotion ? " reduced-motion" : ""}`}
      style={theme.style}
      data-palette={theme.paletteId}
    >
      <Header
        label={phaseLabel(snapshot)}
        menuOpen={menuOpen}
        onMenu={() => setMenuOpen((o) => !o)}
        onHide={() => run(() => backend.hideCompanion())}
      />
      <div className="cw-body">
        <TimerView
          phase={phase}
          session={session}
          awaitingAfterFocus={awaiting?.after === "focus"}
          size={RING_SIZE}
          stroke={6}
          reducedMotion={reducedMotion}
          heading={heading}
          aside={aside}
        >
          <CharacterView
            pack={pack}
            baseKey={baseKey}
            oneShot={motion.oneShot}
            onOneShotEnd={motion.onOneShotEnd}
            hopNonce={motion.hopNonce}
            bubble={motion.bubble}
            reducedMotion={reducedMotion}
            blink
            width={CHARACTER_SIZE}
            height={CHARACTER_SIZE}
            filter={theme.palette.characterFilter}
            label="Your companion. Click to say hi."
            onClick={motion.poke}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenuOpen(true);
            }}
          />
        </TimerView>
      </div>
      <div className="cw-footer">
        <HarvestRow items={snapshot?.today.harvest ?? []} iconSize={13} emptyText="Today's harvest grows here" />
      </div>
      {toast ? (
        <div className="cw-toast" role="status">
          {toast}
        </div>
      ) : null}
      {menuOpen ? (
        <CharacterMenu
          items={replayItems(unlocks, unlocked, pack)}
          focusRunning={focusRunning}
          nextLabel={next ? `${next.label} at ${next.points} points` : null}
          onReplay={(id) => run(() => backend.replayAnimation(id).then(() => setMenuOpen(false)))}
          onSettings={() => {
            setMenuOpen(false);
            run(() => backend.openSettings("companion"));
          }}
          onClose={() => setMenuOpen(false)}
        />
      ) : null}
    </div>
  );
}

function Heading({ title, sub, accent }: { title: string; sub?: string; accent?: boolean }) {
  return (
    <div className="cw-heading">
      <span className="cw-heading__title">{title}</span>
      {sub ? <span className={`cw-heading__sub${accent ? " cw-heading__sub--accent" : ""}`}>{sub}</span> : null}
    </div>
  );
}

function AwaitingHeading({ awaiting, unlocks }: { awaiting: AwaitingView; unlocks: readonly UnlockDef[] }) {
  const minutes = awaiting.record ? wholeMinutes(awaiting.record.activeMs) : null;
  const points = awaiting.award ? awaiting.award.points + awaiting.award.bonus : null;
  const fresh = awaiting.award?.newlyUnlocked ?? [];
  return (
    <div className="cw-heading">
      <span className="cw-heading__title">{minutes !== null ? `${minutes} min together` : "Complete"}</span>
      {points !== null ? <span className="cw-heading__sub cw-heading__sub--accent">+{points} friendship</span> : null}
      {fresh.length > 0 ? (
        <span className="cw-heading__sub cw-heading__new">New: {fresh.map((id) => unlockLabel(unlocks, id)).join(", ")}</span>
      ) : null}
    </div>
  );
}

function StartControls({
  limits,
  lastMinutes,
  busy,
  onStart,
  onCustom,
  onTask,
  onDone,
}: {
  limits: Limits;
  lastMinutes: number;
  busy: boolean;
  onStart: (minutes: number) => void;
  onCustom: () => void;
  onTask: () => void;
  onDone?: () => void;
}) {
  return (
    <div className="cw-actions">
      <WidgetButton variant="primary" icon="play" wide disabled={busy} onPress={() => onStart(lastMinutes)}>
        Start {lastMinutes}
      </WidgetButton>
      <div className="cw-chips">
        {limits.presets.map((m) => (
          <WidgetButton
            key={m}
            variant="chip"
            label={`Start ${m} minutes`}
            disabled={busy}
            active={m === lastMinutes}
            onPress={() => onStart(m)}
          >
            {m}
          </WidgetButton>
        ))}
        <WidgetButton variant="chip" label="Custom length" disabled={busy} onPress={onCustom}>
          ±
        </WidgetButton>
      </div>
      <div className="cw-row cw-row--small">
        <WidgetButton variant="ghost" icon="pencil" disabled={busy} onPress={onTask} label="Add a task in Settings">
          task
        </WidgetButton>
        {onDone ? (
          <WidgetButton variant="ghost" icon="check" disabled={busy} onPress={onDone}>
            done
          </WidgetButton>
        ) : null}
      </div>
    </div>
  );
}

function SessionControls({
  snapshot,
  busy,
  onPause,
  onResume,
  onEnd,
}: {
  snapshot: Snapshot;
  busy: boolean;
  onPause: () => void;
  onResume: () => void;
  onEnd: () => void;
}) {
  const s = snapshot.session;
  if (!s) return null;
  const paused = s.status === "paused";
  const isBreak = s.kind === "break";
  const status = paused
    ? pauseLine(s.pauseReason)
    : isBreak
      ? "Resting"
      : s.taskLabel ?? "Focusing";
  return (
    <div className="cw-session">
      <span className={`cw-status${paused ? " cw-status--paused" : ""}`} title={s.taskLabel ?? undefined}>
        <span className="cw-status__text">{status}</span>
      </span>
      <div className="cw-row">
        {paused ? (
          <WidgetButton variant="primary" icon="play" disabled={busy} onPress={onResume}>
            Resume
          </WidgetButton>
        ) : isBreak ? null : (
          <WidgetButton variant="soft" icon="pause" disabled={busy} onPress={onPause}>
            Pause
          </WidgetButton>
        )}
        {isBreak && !paused ? (
          <WidgetButton variant="outline" icon="stop" wide disabled={busy} onPress={onEnd}>
            End break
          </WidgetButton>
        ) : (
          <WidgetButton variant="outline" disabled={busy} onPress={onEnd} label={isBreak ? "End break" : "End session"}>
            End
          </WidgetButton>
        )}
      </div>
    </div>
  );
}
