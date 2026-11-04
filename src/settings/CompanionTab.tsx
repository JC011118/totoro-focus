// Settings → Companion: friendship progress, palette picker, character packs,
// the unlock gallery (with replays) and the companion position reset.

import { useCallback, useEffect, useState } from "react";
import type { AppInfo } from "../ipc/bindings/AppInfo";
import type { PackInfo } from "../ipc/bindings/PackInfo";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { UnlockDef } from "../ipc/bindings/UnlockDef";
import { errorMessage } from "../ipc/errors";
import { useBackend, useRuntime, useSnapshotCommand } from "../state/BackendContext";
import { autoPaletteId, effectivePalette, pickablePalettes, resolvesIn } from "../theme/choose";
import { presentationTriggers, useLocalNow } from "../theme/localTime";
import type { ResolvedPalette } from "../theme/palettes";
import { Button, Message, Row, Section } from "./ui";

const KIND_LABEL: Record<UnlockDef["kind"], string> = {
  states: "Basics",
  animation: "Animation",
  idleVariant: "Idle variant",
  palette: "Palette",
};

/** Progress from the last reached threshold to the next one, 0..1. */
export function friendshipProgress(total: number, unlocks: readonly UnlockDef[], nextPoints: number | null): number {
  if (nextPoints === null) return 1;
  const reached = unlocks.filter((u) => u.points <= total).reduce((m, u) => Math.max(m, u.points), 0);
  const span = nextPoints - reached;
  return span > 0 ? Math.min(1, Math.max(0, (total - reached) / span)) : 0;
}

const fmt = (n: number) => n.toLocaleString();

export function CompanionTab({
  snapshot,
  appInfo,
  pack,
}: {
  snapshot: Snapshot;
  appInfo: AppInfo | null;
  pack: ResolvedPack | null;
}) {
  const backend = useBackend();
  const command = useSnapshotCommand();
  const runtime = useRuntime();
  const now = useLocalNow();
  const unlocks = appInfo?.unlocks ?? [];
  const f = snapshot.friendship;
  const focusRunning = snapshot.phase === "focus" && snapshot.session?.status === "running";

  const [packs, setPacks] = useState<PackInfo[] | null>(null);
  const [packError, setPackError] = useState<string | null>(null);
  const [message, setMessage] = useState<{ tone: "error" | "ok"; text: string } | null>(null);

  const loadPacks = useCallback(() => {
    backend.listPacks().then(
      (p) => {
        setPacks(p);
        setPackError(null);
      },
      (e: unknown) => setPackError(errorMessage(e)),
    );
  }, [backend]);
  useEffect(() => {
    loadPacks();
  }, [loadPacks]);

  const act = (fn: () => Promise<unknown>, ok?: string) => {
    setMessage(null);
    fn().then(
      () => ok && setMessage({ tone: "ok", text: ok }),
      (e: unknown) => setMessage({ tone: "error", text: errorMessage(e) }),
    );
  };

  const choosePack = (id: string) => {
    setPackError(null);
    backend.setActivePack(id).then(
      (p) => runtime.pack.accept(p),
      (e: unknown) => setPackError(errorMessage(e)),
    );
  };

  const progress = friendshipProgress(f.totalPoints, unlocks, f.next?.points ?? null);
  const autoId = autoPaletteId(unlocks, f.unlocked, presentationTriggers(now, snapshot.today.date));
  const unlockedSet = new Set(f.unlocked);

  return (
    <div className="st-tab">
      <section className="st-friend">
        <div className="st-friend__top">
          <div>
            <div className="st-today__label">Friendship</div>
            <div className="st-friend__points">
              {fmt(f.totalPoints)} <span>points</span>
            </div>
          </div>
          <div className="st-friend__next">
            {f.next ? (
              <>
                Next: <strong>{f.next.label}</strong>
                <br />
                {fmt(f.next.remaining)} to go
              </>
            ) : (
              "Everything is unlocked. Thank you for the company."
            )}
          </div>
        </div>
        <div
          className="st-progress"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(progress * 100)}
          aria-label="Progress to the next unlock"
        >
          <div className="st-progress__fill" style={{ width: `${progress * 100}%` }} />
        </div>
        <p className="st-friend__note">One point for every active minute, plus a small bonus when a session completes. Nothing is ever taken away.</p>
      </section>

      <Section title="Colors">
        <div className="st-palettes" role="radiogroup" aria-label="Palette">
          {pickablePalettes(unlocks, f.unlocked).map((id) => {
            const shownId = id === "auto" ? autoId : id;
            const p = effectivePalette(shownId, unlocks, pack);
            const selected = snapshot.settings.palette === id;
            return (
              <button
                key={id}
                type="button"
                role="radio"
                aria-checked={selected}
                className={`st-palette${selected ? " st-palette--on" : ""}`}
                onClick={() => act(() => command(backend.updateSettings({ palette: id })))}
              >
                <Swatch p={p} />
                <span className="st-palette__name">{id === "auto" ? "Automatic" : p.label}</span>
                <span className="st-palette__hint">{id === "auto" ? `Now: ${p.label}` : id === "forest" ? "Default" : "Unlocked"}</span>
              </button>
            );
          })}
        </div>
        <p className="st-card__note">Automatic follows the time of day and the season with the palettes you have unlocked.</p>
      </Section>

      <Section
        title="Character pack"
        aside={
          <Button variant="quiet" onClick={loadPacks}>
            Rescan
          </Button>
        }
      >
        {pack?.fallbackFrom ? (
          <Message>
            “{pack.fallbackFrom}” isn't available right now, so {pack.info.name} is shown. Your choice is kept.
          </Message>
        ) : null}
        {packs === null && !packError ? <p className="st-empty">Looking for packs…</p> : null}
        <ul className="st-packs">
          {(packs ?? []).map((p) => {
            const active = snapshot.settings.activePack === p.id;
            return (
              <li key={`${p.source}:${p.id}`} className={`st-pack${active ? " st-pack--on" : ""}${p.valid ? "" : " st-pack--invalid"}`}>
                <div className="st-pack__main">
                  <div className="st-pack__name">
                    {p.name}
                    <span className="st-badge">{p.source === "bundled" ? "built in" : p.source === "dev" ? "dev" : "yours"}</span>
                    {p.formatKind === "petdex" ? <span className="st-badge">Petdex</span> : null}
                  </div>
                  <div className="st-pack__meta">
                    {[p.author ? `by ${p.author}` : null, p.license ?? "no license declared"].filter(Boolean).join(" · ")}
                  </div>
                  {p.errors.map((e) => (
                    <div key={e} className="st-pack__err">
                      {e}
                    </div>
                  ))}
                  {p.warnings.map((w) => (
                    <div key={w} className="st-pack__warn">
                      {w}
                    </div>
                  ))}
                </div>
                <div className="st-pack__action">
                  {active ? (
                    <span className="st-pack__using">In use</span>
                  ) : (
                    <Button disabled={!p.valid} onClick={() => choosePack(p.id)}>
                      Use
                    </Button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
        {packError ? <Message tone="error">{packError}</Message> : null}
        {appInfo ? (
          <p className="st-card__note">
            Add your own packs (a folder with <code>pack.json</code>, or a Petdex <code>pet.json</code>) to{" "}
            <code className="st-path">{appInfo.userPacksDir}</code>, then Rescan.
          </p>
        ) : null}
      </Section>

      <Section title="Unlocks">
        {focusRunning ? <Message>Replays wait until this focus session ends.</Message> : null}
        <div className="st-gallery">
          {unlocks.map((u) => {
            const isUnlocked = unlockedSet.has(u.id);
            const playable = u.kind === "animation" || u.kind === "idleVariant";
            const inPack = resolvesIn(pack, u.id);
            return (
              <div key={u.id} className={`st-unlock${isUnlocked ? "" : " st-unlock--locked"}`}>
                <div className="st-unlock__top">
                  <span className="st-unlock__kind">{KIND_LABEL[u.kind]}</span>
                  <span className="st-unlock__pts">{isUnlocked ? "unlocked" : `${fmt(u.points)} pts`}</span>
                </div>
                <div className="st-unlock__label">{u.label}</div>
                <div className="st-unlock__desc">{u.description}</div>
                {isUnlocked && playable ? (
                  inPack ? (
                    <Button
                      variant="quiet"
                      icon="play"
                      disabled={focusRunning}
                      onClick={() => act(() => backend.replayAnimation(u.id), `Playing “${u.label}” on the companion.`)}
                    >
                      Replay
                    </Button>
                  ) : (
                    <span className="st-unlock__na">Not in this pack</span>
                  )
                ) : null}
                {isUnlocked && u.kind === "palette" && snapshot.settings.palette !== u.id ? (
                  <Button variant="quiet" onClick={() => act(() => command(backend.updateSettings({ palette: u.id })))}>
                    Use colors
                  </Button>
                ) : null}
              </div>
            );
          })}
        </div>
        {message ? <Message tone={message.tone}>{message.text}</Message> : null}
      </Section>

      <Section title="Position">
        <Row label="Companion position" hint="Move the companion back to its starting spot on the main display.">
          <Button onClick={() => act(() => backend.resetCompanionPosition(), "Moved the companion back.")}>Reset</Button>
        </Row>
      </Section>
    </div>
  );
}

function Swatch({ p }: { p: ResolvedPalette }) {
  return (
    <span className="st-swatch" style={{ background: p.background, borderColor: p.ringTrack }} aria-hidden="true">
      <span style={{ background: p.surface }} />
      <span style={{ background: p.ring }} />
      <span style={{ background: p.accent }} />
      <span style={{ background: p.text }} />
    </span>
  );
}
