import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useEffectEvent,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import type { Backend, BackendEventName, BackendEvents, Unlisten } from "../ipc/backend";
import type { AppInfo } from "../ipc/bindings/AppInfo";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { AppInfoStore, PackStore, SnapshotStore } from "./stores";

/** Per-window renderer state: one set of caches over one Backend. */
export interface Runtime {
  backend: Backend;
  snapshots: SnapshotStore;
  pack: PackStore;
  appInfo: AppInfoStore;
}

const RuntimeContext = createContext<Runtime | null>(null);

export function createRuntime(backend: Backend): Runtime {
  return {
    backend,
    snapshots: new SnapshotStore(backend),
    pack: new PackStore(backend),
    appInfo: new AppInfoStore(backend),
  };
}

/**
 * Provides the backend and connects the caches (subscribe, then fetch). Each
 * window has its own provider; the dev preview mounts two over one mock.
 */
export function BackendProvider({ backend, children }: { backend: Backend; children: ReactNode }) {
  const [runtime] = useState(() => createRuntime(backend));
  useEffect(() => runtime.snapshots.connect(), [runtime]);
  useEffect(() => runtime.pack.connect(), [runtime]);
  useEffect(() => runtime.appInfo.connect(), [runtime]);
  return <RuntimeContext.Provider value={runtime}>{children}</RuntimeContext.Provider>;
}

export function useRuntime(): Runtime {
  const r = useContext(RuntimeContext);
  if (!r) throw new Error("useRuntime outside <BackendProvider>");
  return r;
}

export function useBackend(): Backend {
  return useRuntime().backend;
}

export function useSnapshot(): Snapshot | null {
  const { snapshots } = useRuntime();
  return useSyncExternalStore(snapshots.subscribe, snapshots.get);
}

export function useActivePack(): ResolvedPack | null {
  const { pack } = useRuntime();
  return useSyncExternalStore(pack.subscribe, pack.get);
}

export function useAppInfo(): AppInfo | null {
  const { appInfo } = useRuntime();
  return useSyncExternalStore(appInfo.subscribe, appInfo.get);
}

/** Runs a command that returns a Snapshot and applies the result (seq-ordered). */
export function useSnapshotCommand(): (call: Promise<Snapshot>) => Promise<Snapshot> {
  const { snapshots } = useRuntime();
  return useCallback(
    async (call: Promise<Snapshot>) => {
      const s = await call;
      snapshots.offer(s);
      return s;
    },
    [snapshots],
  );
}

/** Subscribes to a backend event for the component's lifetime. */
/**
 * Subscribe to a backend event. `onReady` runs once the listener is registered,
 * which is where subscribe-then-fetch work belongs (fetching earlier can miss an
 * event that arrives between the fetch and the subscription).
 */
export function useBackendEvent<K extends BackendEventName>(
  name: K,
  handler: (payload: BackendEvents[K]) => void,
  onReady?: () => void,
): void {
  const backend = useBackend();
  const onEvent = useEffectEvent(handler);
  const ready = useEffectEvent(() => onReady?.());
  useEffect(() => {
    let active = true;
    let unlisten: Unlisten | null = null;
    backend
      .on(name, (p) => {
        if (active) onEvent(p);
      })
      .then(
        (u) => {
          if (!active) {
            u();
            return;
          }
          unlisten = u;
          ready();
        },
        (e: unknown) => {
          console.warn(`[focus] could not listen for ${name}`, e);
          if (active) ready();
        },
      );
    return () => {
      active = false;
      unlisten?.();
    };
  }, [backend, name]);
}
