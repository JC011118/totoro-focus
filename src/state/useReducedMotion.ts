import { useSyncExternalStore } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";

function media(): MediaQueryList | null {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia(QUERY) : null;
}

function subscribe(onChange: () => void): () => void {
  const m = media();
  if (!m) return () => {};
  m.addEventListener("change", onChange);
  return () => m.removeEventListener("change", onChange);
}

const getSystem = (): boolean => media()?.matches ?? false;

/** The OS-level "Reduce motion" preference. */
export function usePrefersReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, getSystem, () => false);
}

/** SPEC §7.1: reduced motion = settings.reducedMotion || prefers-reduced-motion. */
export function useEffectiveReducedMotion(setting: boolean | undefined): boolean {
  const system = usePrefersReducedMotion();
  return Boolean(setting) || system;
}
