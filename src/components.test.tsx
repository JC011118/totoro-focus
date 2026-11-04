import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CharacterView } from "./character/CharacterView";
import { StaticCompanion } from "./character/StaticCompanion";
import { HarvestRow } from "./harvest/HarvestRow";
import type { HarvestItem } from "./ipc/bindings/HarvestItem";
import { anim, makePack, session } from "./test/fixtures";
import { ringFraction, TimerView } from "./timer/TimerView";

describe("TimerView", () => {
  it("shows the backend's remainingLabel verbatim and never computes time", () => {
    // remainingMs deliberately disagrees with the label: the label wins.
    const s = session({ remainingLabel: "24:13", remainingMs: 999 });
    render(<TimerView phase="focus" session={s} size={100} reducedMotion={false} />);
    expect(screen.getByRole("timer").textContent).toBe("24:13");
  });

  it("draws the ring from remainingMs / plannedMs, shrinking to empty", () => {
    expect(ringFraction(session({ plannedMs: 1_500_000, remainingMs: 1_500_000 }))).toBe(1);
    expect(ringFraction(session({ plannedMs: 1_500_000, remainingMs: 750_000 }))).toBe(0.5);
    expect(ringFraction(session({ plannedMs: 1_500_000, remainingMs: 0 }))).toBe(0);
    expect(ringFraction(session({ plannedMs: 0, remainingMs: 5 }))).toBe(0);
    expect(ringFraction(null)).toBe(0);
    const { container, rerender } = render(
      <TimerView phase="focus" session={session({ plannedMs: 1000, remainingMs: 250 })} size={100} reducedMotion={false} />,
    );
    const arc = container.querySelector(".ring__arc");
    expect(arc?.getAttribute("data-fraction")).toBe("0.25");
    expect(arc?.classList.contains("ring__arc--animate")).toBe(true);
    rerender(<TimerView phase="focus" session={session({ plannedMs: 1000, remainingMs: 250 })} size={100} reducedMotion />);
    expect(container.querySelector(".ring__arc")?.classList.contains("ring__arc--animate")).toBe(false);
    rerender(<TimerView phase="idle" session={null} size={100} reducedMotion={false} heading={<span>Ready</span>} />);
    expect(container.querySelector(".ring__arc")).toBeNull();
    expect(screen.queryByRole("timer")).toBeNull();
    expect(screen.getByText("Ready")).toBeTruthy();
  });

  it("marks paused and break looks, and narrows 100+ minute labels", () => {
    const { container, rerender } = render(
      <TimerView phase="focus" session={session({ status: "paused" })} size={100} reducedMotion={false} />,
    );
    expect(container.querySelector(".ring--paused")).toBeTruthy();
    rerender(<TimerView phase="break" session={session({ kind: "break" })} size={100} reducedMotion={false} />);
    expect(container.querySelector(".ring--break")).toBeTruthy();
    rerender(<TimerView phase="focus" session={session({ remainingLabel: "180:00" })} size={100} reducedMotion={false} />);
    expect(screen.getByRole("timer").classList.contains("countdown--wide")).toBe(true);
  });
});

describe("HarvestRow", () => {
  const items = (n: number): HarvestItem[] => Array.from({ length: n }, (_, i) => (i % 3 === 2 ? "leaf" : "acorn"));

  it("shows the first 12 then a +N chip", () => {
    const { container } = render(<HarvestRow items={items(17)} />);
    expect(container.querySelectorAll(".harvest__item")).toHaveLength(12);
    expect(container.querySelector(".harvest__more")?.textContent).toBe("+5");
    expect(container.querySelectorAll(".harvest__item--leaf")).toHaveLength(4);
    expect(screen.getByRole("img").getAttribute("aria-label")).toBe(
      "Today's harvest: 12 acorns (completed), 5 leaves (ended early)",
    );
  });

  it("has no chip at 12 and an empty text at 0", () => {
    const { container, rerender } = render(<HarvestRow items={items(12)} />);
    expect(container.querySelector(".harvest__more")).toBeNull();
    rerender(<HarvestRow items={[]} emptyText="Nothing yet" />);
    expect(screen.getByText("Nothing yet")).toBeTruthy();
  });
});

describe("CharacterView", () => {
  it("draws the built-in static companion when the pack has no art", () => {
    const { container } = render(
      <CharacterView pack={makePack([])} baseKey="focus" reducedMotion={false} width={74} height={74} label="c" />,
    );
    const svg = container.querySelector(".static-companion");
    expect(svg?.getAttribute("data-mood")).toBe("focus");
    expect(container.querySelector(".sprite")).toBeNull();
    const none = render(<CharacterView pack={null} baseKey="idle" reducedMotion={false} width={74} height={74} label="c" />);
    expect(none.container.querySelector(".static-companion")).toBeTruthy();
  });

  it("renders sprites as CSS backgrounds with percentage math", () => {
    const pack = makePack(["idle"]);
    pack.animations.idle = anim("idle", { frames: 4, sheetColumns: 8, sheetRows: 9, row: 3 });
    const { container } = render(
      <CharacterView pack={pack} baseKey="idle" reducedMotion={false} width={74} height={74} label="c" />,
    );
    const sprite = container.querySelector<HTMLElement>(".sprite");
    expect(sprite?.style.backgroundSize).toBe("800% 900%");
    expect(sprite?.style.backgroundPosition).toBe("0% 37.5%");
    expect(sprite?.style.backgroundImage).toContain("pack://localhost/test/idle.svg");
    expect(container.querySelector("svg")).toBeNull(); // never inline SVG for pack art
  });

  it("falls back to idle art for a base key the pack lacks", () => {
    const { container } = render(
      <CharacterView pack={makePack(["idle"])} baseKey="paused" reducedMotion={false} width={74} height={74} label="c" />,
    );
    expect(container.querySelector(".sprite")?.getAttribute("data-anim")).toBe("idle");
  });

  it("shows the still frame under reduced motion", () => {
    const pack = makePack(["completed"]);
    pack.animations.completed = anim("completed", { frames: 6, stillFrame: 5, loop: false, sheetColumns: 6 });
    const { container } = render(
      <CharacterView pack={pack} baseKey="completed" reducedMotion width={74} height={74} label="c" />,
    );
    const sprite = container.querySelector<HTMLElement>(".sprite");
    expect(sprite?.getAttribute("data-frame")).toBe("5");
    expect(sprite?.style.backgroundPosition).toBe("100% 0%");
  });

  it("is a tabIndex=-1 button when clickable, and shows a static bubble", () => {
    const onClick = vi.fn();
    render(
      <CharacterView
        pack={null}
        baseKey="idle"
        reducedMotion
        bubble="hi!"
        width={74}
        height={74}
        label="Your companion"
        onClick={onClick}
      />,
    );
    const btn = screen.getByRole("button", { name: "Your companion" });
    expect(btn.tabIndex).toBe(-1);
    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toBe("hi!");
  });

  it("plays a one-shot over the base and ends it at most after the cap", async () => {
    vi.useFakeTimers();
    try {
      const pack = makePack(["idle", "wave"]);
      pack.animations.wave = anim("wave", { frames: 2, fps: 10, loop: false, sheetColumns: 2 });
      const onEnd = vi.fn();
      const { container } = render(
        <CharacterView
          pack={pack}
          baseKey="idle"
          oneShot={{ key: "wave", nonce: 1 }}
          onOneShotEnd={onEnd}
          reducedMotion={false}
          width={74}
          height={74}
          label="c"
        />,
      );
      expect(container.querySelector(".sprite")?.getAttribute("data-anim")).toBe("wave");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(onEnd).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("under reduced motion a one-shot never shows: the base still stays and the shot ends at once", () => {
    const onEnd = vi.fn();
    const pack = makePack(["idle", "wave"]);
    const { container } = render(
      <CharacterView
        pack={pack}
        baseKey="idle"
        oneShot={{ key: "wave", nonce: 1 }}
        onOneShotEnd={onEnd}
        reducedMotion
        width={74}
        height={74}
        label="c"
      />,
    );
    const sprite = container.querySelector(".sprite");
    expect(sprite?.getAttribute("data-anim")).toBe("idle");
    expect(sprite?.getAttribute("data-frame")).toBe(String(pack.animations.idle?.stillFrame));
    expect(onEnd).toHaveBeenCalledTimes(1);
  });

  it("ends a one-shot without art immediately", () => {
    const onEnd = vi.fn();
    render(
      <CharacterView
        pack={makePack(["idle"])}
        baseKey="idle"
        oneShot={{ key: "celebrate", nonce: 1 }}
        onOneShotEnd={onEnd}
        reducedMotion={false}
        width={74}
        height={74}
        label="c"
      />,
    );
    expect(onEnd).toHaveBeenCalledTimes(1);
  });

  it("static companion moods look different per state", () => {
    const { container, rerender } = render(<StaticCompanion mood="idle" size={74} />);
    const idle = container.innerHTML;
    rerender(<StaticCompanion mood="completed" size={74} />);
    expect(container.innerHTML).not.toBe(idle);
  });
});
