import { afterEach, describe, expect, it, vi } from "vitest";
import type { Backend, Unlisten } from "../ipc/backend";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { APP_INFO, FakeBackend, flush, makePack, makeSnapshot } from "../test/fixtures";
import { AppInfoStore, PackStore, shouldApply, SnapshotStore } from "./stores";

const snap = (seq: number, extra: Partial<Snapshot> = {}) => makeSnapshot({ seq, ...extra });

afterEach(() => {
  vi.useRealTimers();
});

describe("shouldApply (seq ordering)", () => {
  it("applies only strictly newer snapshots", () => {
    expect(shouldApply(null, snap(0))).toBe(true);
    expect(shouldApply(snap(5), snap(6))).toBe(true);
    expect(shouldApply(snap(5), snap(5))).toBe(false);
    expect(shouldApply(snap(5), snap(4))).toBe(false);
  });
});

describe("SnapshotStore", () => {
  it("ignores stale and duplicate snapshots from commands and events", () => {
    const store = new SnapshotStore(new FakeBackend());
    const seen: number[] = [];
    store.subscribe(() => seen.push(store.get()!.seq));
    expect(store.offer(snap(3))).toBe(true);
    expect(store.offer(snap(2))).toBe(false); // a late command reply
    expect(store.offer(snap(3))).toBe(false); // the same snapshot as event and reply
    expect(store.offer(snap(4))).toBe(true);
    expect(seen).toEqual([3, 4]);
    expect(store.get()?.seq).toBe(4);
  });

  it("subscribes to focus://snapshot before calling get_snapshot", async () => {
    const backend = new FakeBackend();
    const store = new SnapshotStore(backend);
    const stop = store.connect();
    await flush();
    expect(backend.calls).toEqual(["on:snapshot", "getSnapshot"]);
    expect(store.get()?.seq).toBe(1);
    stop();
  });

  it("keeps a newer event that arrives before the fetch resolves", async () => {
    let resolveFetch: (s: Snapshot) => void = () => {};
    const backend = new FakeBackend();
    backend.getSnapshot.mockImplementation(
      () =>
        new Promise<Snapshot>((r) => {
          backend.calls.push("getSnapshot");
          resolveFetch = r;
        }),
    );
    const store = new SnapshotStore(backend);
    const stop = store.connect();
    await flush();
    backend.emit("snapshot", snap(8, { phase: "focus" }));
    resolveFetch(snap(7)); // produced earlier by the shell, delivered later
    await flush();
    expect(store.get()?.seq).toBe(8);
    expect(store.get()?.phase).toBe("focus");
    stop();
  });

  it("stops listening on disconnect, even if it happens before subscription resolves", async () => {
    const backend = new FakeBackend();
    const store = new SnapshotStore(backend);
    const stop = store.connect();
    stop(); // before the on() promise settles
    await flush();
    expect(backend.listenerCount("snapshot")).toBe(0);
    expect(backend.getSnapshot).not.toHaveBeenCalled();
    backend.emit("snapshot", snap(9));
    expect(store.get()).toBeNull();
  });

  it("retries get_snapshot after a failure", async () => {
    vi.useFakeTimers();
    const backend = new FakeBackend();
    backend.getSnapshot.mockRejectedValueOnce({ code: "unknown", message: "not ready" });
    const store = new SnapshotStore(backend);
    const onError = vi.fn();
    const stop = store.connect(onError);
    await vi.advanceTimersByTimeAsync(0);
    expect(onError).toHaveBeenCalledTimes(1);
    expect(store.get()).toBeNull();
    await vi.advanceTimersByTimeAsync(2000);
    expect(store.get()?.seq).toBe(1);
    stop();
  });
});

describe("PackStore", () => {
  it("subscribes to pack-changed first and lets a pushed pack win over the initial fetch", async () => {
    let resolveFetch: (p: ReturnType<typeof makePack>) => void = () => {};
    const backend = new FakeBackend();
    backend.getActivePack.mockImplementation(
      () =>
        new Promise((r) => {
          backend.calls.push("getActivePack");
          resolveFetch = r;
        }),
    );
    const store = new PackStore(backend);
    const stop = store.connect();
    await flush();
    expect(backend.calls).toEqual(["on:packChanged", "getActivePack"]);
    const pushed = makePack(["idle", "wave"]);
    backend.emit("packChanged", pushed);
    resolveFetch(makePack(["idle"]));
    await flush();
    expect(store.get()).toBe(pushed);
    stop();
  });
});

describe("AppInfoStore", () => {
  it("fetches once", async () => {
    const backend = new FakeBackend();
    const store = new AppInfoStore(backend);
    const stop = store.connect();
    await flush();
    expect(store.get()?.name).toBe("Totoro Focus");
    stop();
    store.connect()();
    expect(backend.getAppInfo).toHaveBeenCalledTimes(1);
  });

  it("refresh() refetches (hotkeyError changes after set_hotkey) and keeps the value on failure", async () => {
    const backend = new FakeBackend();
    backend.getAppInfo.mockResolvedValueOnce({ ...APP_INFO, hotkeyError: "taken" });
    const store = new AppInfoStore(backend);
    const stop = store.connect();
    await flush();
    expect(store.get()?.hotkeyError).toBe("taken");
    const changes = vi.fn();
    store.subscribe(changes);
    await store.refresh();
    expect(store.get()?.hotkeyError).toBeNull();
    expect(changes).toHaveBeenCalledTimes(1);
    const onError = vi.fn();
    backend.getAppInfo.mockRejectedValueOnce(new Error("gone"));
    await store.refresh(onError);
    expect(onError).toHaveBeenCalledTimes(1);
    expect(store.get()?.hotkeyError).toBeNull();
    stop();
  });

  it("does not call a command the window lacks after failing", async () => {
    // A minimal backend proving the store needs only getAppInfo.
    const getAppInfo = vi.fn(() => Promise.reject(new Error("denied")));
    const store = new AppInfoStore({ getAppInfo, on: () => Promise.resolve((() => {}) as Unlisten) } as unknown as Backend);
    const onError = vi.fn();
    const stop = store.connect(onError);
    await flush();
    stop();
    expect(onError).toHaveBeenCalledWith(expect.any(Error));
  });
});
