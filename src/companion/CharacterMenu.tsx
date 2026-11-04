// The small character menu (buttons only): replay unlocked animations and
// idle variants. Ids the active pack cannot draw are listed, dimmed, as "not
// in this pack". The backend validates every replay (FocusRunning, NotUnlocked).

import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { UnlockDef } from "../ipc/bindings/UnlockDef";
import { resolvesIn } from "../theme/choose";
import { WidgetButton } from "./WidgetButton";

export interface ReplayItem {
  id: string;
  label: string;
  available: boolean;
}

/**
 * Unlocked animation / idle-variant ids with whether the pack resolves them:
 * playable ones first, then the ones this pack has no art for (each group in
 * table order).
 */
export function replayItems(
  unlocks: readonly UnlockDef[],
  unlocked: readonly string[],
  pack: ResolvedPack | null,
): ReplayItem[] {
  const have = new Set(unlocked);
  const items = unlocks
    .filter((u) => (u.kind === "animation" || u.kind === "idleVariant") && have.has(u.id))
    .map((u) => ({ id: u.id, label: u.label, available: resolvesIn(pack, u.id) }));
  return [...items.filter((i) => i.available), ...items.filter((i) => !i.available)];
}

export function CharacterMenu({
  items,
  focusRunning,
  nextLabel,
  onReplay,
  onSettings,
  onClose,
}: {
  items: ReplayItem[];
  focusRunning: boolean;
  /** e.g. "A small wave at 25 points" when nothing is unlocked yet. */
  nextLabel: string | null;
  onReplay: (id: string) => void;
  onSettings: () => void;
  onClose: () => void;
}) {
  const anyMissing = items.some((i) => !i.available);
  return (
    <div className="cw-menu" role="dialog" aria-label="Character menu">
      <div className="cw-menu__head">
        <span className="cw-menu__title">Replay</span>
        <WidgetButton variant="ghost" onPress={onSettings} className="cw-menu__settings">
          Companion settings…
        </WidgetButton>
        <WidgetButton variant="icon" icon="close" label="Close menu" onPress={onClose} />
      </div>
      <div className="cw-menu__body">
        {items.length === 0 ? (
          <p className="cw-menu__empty">
            New animations appear here as you spend time together.
            {nextLabel ? <span className="cw-menu__next"> Next: {nextLabel}.</span> : null}
          </p>
        ) : (
          <div className={`cw-menu__grid${items.length > 8 ? " cw-menu__grid--dense" : ""}`}>
            {items.map((i) => (
              <WidgetButton
                key={i.id}
                variant="chip"
                icon={i.available ? "play" : undefined}
                className={i.available ? "cw-replay" : "cw-replay cw-replay--missing"}
                label={i.available ? `Replay ${i.label}` : `${i.label} (not in this pack)`}
                disabled={!i.available || focusRunning}
                onPress={() => onReplay(i.id)}
              >
                {i.label}
              </WidgetButton>
            ))}
          </div>
        )}
        {focusRunning && items.length > 0 ? (
          <p className="cw-menu__hint">Replays wait until this session ends.</p>
        ) : anyMissing ? (
          <p className="cw-menu__hint">Dimmed: not in this pack.</p>
        ) : null}
      </div>
    </div>
  );
}
