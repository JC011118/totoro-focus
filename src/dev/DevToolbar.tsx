import { useEffect, useState } from "react";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { DEV_UNLOCKS } from "./devData";
import type { DevPreset, MockBackend } from "./mockBackend";

const PRESETS: { id: DevPreset; label: string }[] = [
  { id: "fresh", label: "Fresh (0 pts)" },
  { id: "idle", label: "Idle" },
  { id: "running", label: "Running" },
  { id: "paused", label: "Paused (slept)" },
  { id: "break", label: "Break" },
  { id: "awaitingFocus", label: "Done: focus" },
  { id: "awaitingBreak", label: "Done: break" },
  { id: "manyHarvest", label: "Many harvest" },
  { id: "highPoints", label: "High points" },
  { id: "long", label: "180 min" },
];

const PALETTES = ["auto", "forest", ...DEV_UNLOCKS.filter((u) => u.kind === "palette").map((u) => u.id)];
const SPEEDS = [1, 60, 600];

export function DevToolbar({ mock }: { mock: MockBackend }) {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  useEffect(() => {
    let un: (() => void) | undefined;
    void mock.on("snapshot", setSnap).then((u) => (un = u));
    void mock.getSnapshot().then(setSnap);
    return () => un?.();
  }, [mock]);
  const [speed, setSpeed] = useState(mock.speed);

  const settings = snap?.settings;
  return (
    <div className="dev-bar" role="toolbar" aria-label="Dev preview controls">
      <div className="dev-bar__group">
        <span className="dev-bar__label">State</span>
        {PRESETS.map((p) => (
          <button key={p.id} type="button" className="dev-btn" onClick={() => mock.preset(p.id)}>
            {p.label}
          </button>
        ))}
      </div>
      <div className="dev-bar__group">
        <span className="dev-bar__label">Show</span>
        <label className="dev-check">
          <input
            type="checkbox"
            checked={settings?.reducedMotion ?? false}
            onChange={(e) => mock.setReducedMotion(e.target.checked)}
          />
          Reduced motion
        </label>
        <label className="dev-check">
          <input
            type="checkbox"
            checked={settings?.activePack === "missing-art"}
            onChange={(e) => void mock.setActivePack(e.target.checked ? "missing-art" : "fixtures")}
          />
          Missing art
        </label>
        <label className="dev-check">
          Palette
          <select value={settings?.palette ?? "auto"} onChange={(e) => mock.setPalette(e.target.value)}>
            {PALETTES.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>
        <span className="dev-bar__label">Speed</span>
        {SPEEDS.map((s) => (
          <button
            key={s}
            type="button"
            className={`dev-btn${speed === s ? " dev-btn--on" : ""}`}
            onClick={() => {
              mock.setSpeed(s);
              setSpeed(s);
            }}
          >
            ×{s}
          </button>
        ))}
        <button type="button" className="dev-btn" onClick={() => mock.simulateSleep()}>
          Sleep Mac
        </button>
        <button type="button" className="dev-btn" onClick={() => mock.openPicker()}>
          Tray: custom…
        </button>
        <span className="dev-bar__info">
          {snap ? `seq ${snap.seq} · rev ${snap.revision} · ${snap.friendship.totalPoints} pts · ${snap.phase}` : "…"}
          {mock.hotkeyCaptureActive ? " · hotkey capture ON" : ""}
        </span>
      </div>
    </div>
  );
}
