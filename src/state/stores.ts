// Renderer-side caches of backend state. They hold what the backend last said
// and never derive time. SPEC §1/§10: subscribe to events first, then fetch,
// and apply a Snapshot only when `seq > lastSeq`.

import type { Backend, Unlisten } from "../ipc/backend";
import type { AppInfo } from "../ipc/bindings/AppInfo";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { Snapshot } from "../ipc/bindings/Snapshot";

/** True iff `next` is newer than what we hold. Equal seq = duplicate, ignored. */
export function shouldApply(current: Snapshot | null, next: Snapshot): boolean {
  return current === null || next.seq > current.seq;
}

type Listener = () => void;

export interface Store<T> {
  get(): T;
  subscribe(listener: Listener): () => void;
}

class Emitter<T> implements Store<T> {
  private listeners = new Set<Listener>();
  constructor(protected value: T) {}
  get = (): T => this.value;
  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  protected set(value: T): void {
    if (Object.is(value, this.value)) return;
    this.value = value;
    for (const l of [...this.listeners]) l();
  }
}

const RETRY_MS = 2000;

/**
 * Registers `subscribe` first; only once the listener is live does it call
 * `fetch`, so no update can fall between the two. Retries a failed fetch.
 * Returns a disconnect function (safe to call before setup has finished).
 */
function connectSubscribeThenFetch<T>(
  subscribe: (onEvent: (v: T) => void) => Promise<Unlisten>,
  fetch: () => Promise<T>,
  onEvent: (v: T) => void,
  onFetched: (v: T) => void,
  onError: (e: unknown) => void,
): () => void {
  let active = true;
  let unlisten: Unlisten | null = null;
  let retry: ReturnType<typeof setTimeout> | undefined;

  const doFetch = () => {
    fetch().then(
      (v) => {
        if (active) onFetched(v);
      },
      (e: unknown) => {
        if (!active) return;
        onError(e);
        retry = setTimeout(doFetch, RETRY_MS);
      },
    );
  };
  const doSubscribe = () => {
    subscribe((v) => {
      if (active) onEvent(v);
    }).then(
      (u) => {
        if (!active) {
          u();
          return;
        }
        unlisten = u;
        doFetch();
      },
      (e: unknown) => {
        if (!active) return;
        onError(e);
        retry = setTimeout(doSubscribe, RETRY_MS);
      },
    );
  };
  doSubscribe();

  return () => {
    active = false;
    clearTimeout(retry);
    unlisten?.();
    unlisten = null;
  };
}

export class SnapshotStore extends Emitter<Snapshot | null> {
  constructor(private backend: Backend) {
    super(null);
  }

  /** Apply `s` if it is newer than the current snapshot. Returns whether it was applied. */
  offer = (s: Snapshot): boolean => {
    if (!shouldApply(this.value, s)) return false;
    this.set(s);
    return true;
  };

  connect(onError: (e: unknown) => void = logError("snapshot")): () => void {
    return connectSubscribeThenFetch<Snapshot>(
      (cb) => this.backend.on("snapshot", cb),
      () => this.backend.getSnapshot(),
      this.offer,
      this.offer,
      onError,
    );
  }
}

/** The active pack. Pack changes carry no seq: a pushed event always wins over the initial fetch. */
export class PackStore extends Emitter<ResolvedPack | null> {
  private sawEvent = false;
  constructor(private backend: Backend) {
    super(null);
  }

  accept = (p: ResolvedPack): void => {
    this.set(p);
  };

  connect(onError: (e: unknown) => void = logError("pack")): () => void {
    this.sawEvent = false;
    return connectSubscribeThenFetch<ResolvedPack>(
      (cb) => this.backend.on("packChanged", cb),
      () => this.backend.getActivePack(),
      (p) => {
        this.sawEvent = true;
        this.set(p);
      },
      (p) => {
        if (!this.sawEvent) this.set(p);
      },
      onError,
    );
  }
}

/** Static app facts; fetched once (retried until it succeeds). */
export class AppInfoStore extends Emitter<AppInfo | null> {
  constructor(private backend: Backend) {
    super(null);
  }

  connect(onError: (e: unknown) => void = logError("app info")): () => void {
    let active = true;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const load = () => {
      this.backend.getAppInfo().then(
        (info) => {
          if (active) this.set(info);
        },
        (e: unknown) => {
          if (!active) return;
          onError(e);
          retry = setTimeout(load, RETRY_MS);
        },
      );
    };
    if (this.value === null) load();
    return () => {
      active = false;
      clearTimeout(retry);
    };
  }
}

function logError(what: string) {
  return (e: unknown) => console.warn(`[focus] could not load ${what}`, e);
}
