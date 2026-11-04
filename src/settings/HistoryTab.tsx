// Settings → History. Timer records, not proof of attention: the wording
// stays neutral. Refetches on mount and on focusCompleted / focusInterrupted
// (SPEC §10).

import { useCallback, useState } from "react";
import { HarvestRow } from "../harvest/HarvestRow";
import { AcornIcon, LeafIcon } from "../harvest/icons";
import type { SessionRecord } from "../ipc/bindings/SessionRecord";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { errorMessage } from "../ipc/errors";
import { countHarvest } from "../format/harvest";
import { activeMinutesLabel, clockTime, groupByDay } from "../format/history";
import { useBackend, useBackendEvent } from "../state/BackendContext";
import { Message, Section } from "./ui";

export const HISTORY_LIMIT = 50;

export function HistoryTab({ snapshot }: { snapshot: Snapshot }) {
  const backend = useBackend();
  const [records, setRecords] = useState<SessionRecord[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refetch = useCallback(() => {
    backend.getHistory(HISTORY_LIMIT).then(
      (r) => {
        setRecords(r);
        setError(null);
      },
      (e: unknown) => setError(errorMessage(e)),
    );
  }, [backend]);

  // Subscribe first, then fetch once the listener is live, so a completion that
  // lands in between is never missed.
  useBackendEvent(
    "transition",
    (t) => {
      if (t.type === "focusCompleted" || t.type === "focusInterrupted") refetch();
    },
    refetch,
  );

  const today = snapshot.today;
  const { completed, interrupted } = countHarvest(today.harvest);
  const groups = records ? groupByDay(records, today.date) : [];

  return (
    <div className="st-tab">
      <section className="st-today">
        <div className="st-today__label">Today</div>
        <div className="st-today__value">{today.focusLabel}</div>
        <div className="st-today__sub">
          {completed} completed · {interrupted} ended early
          {today.activeFocusMs > 0 ? " · includes the current session" : ""}
        </div>
        <div className="st-today__harvest">
          <HarvestRow items={today.harvest} iconSize={18} emptyText="Nothing harvested yet today." />
        </div>
      </section>

      <Section title="Recent sessions">
        {error ? <Message tone="error">{error}</Message> : null}
        {records === null && !error ? <p className="st-empty">Loading…</p> : null}
        {records !== null && records.length === 0 ? (
          <p className="st-empty">No sessions yet. Your first one will appear here.</p>
        ) : null}
        {groups.map((g) => (
          <div key={g.dateKey || "unknown"} className="st-day">
            <div className="st-day__label">{g.label}</div>
            <ul className="st-sessions">
              {g.records.map((r) => (
                <li key={r.id} className="st-session">
                  <span className="st-session__icon">
                    {r.outcome === "completed" ? (
                      <AcornIcon size={16} title="Completed" />
                    ) : (
                      <LeafIcon size={16} title="Ended early" />
                    )}
                  </span>
                  <span className="st-session__time">
                    {clockTime(r.startedAt, r.endedUtcOffsetMinutes)}–{clockTime(r.endedAt, r.endedUtcOffsetMinutes)}
                  </span>
                  <span className={`st-session__task${r.taskLabel ? "" : " st-session__task--none"}`}>
                    {r.taskLabel ?? "No task label"}
                  </span>
                  <span className="st-session__mins">{activeMinutesLabel(r.activeMs)}</span>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </Section>
      <p className="st-footnote">
        These are timer records: they show active time on the clock, not how focused anyone was. Paused time
        and time asleep are never counted.
      </p>
    </div>
  );
}
