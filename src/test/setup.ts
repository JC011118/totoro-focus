import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Vitest runs without globals, so Testing Library cannot auto-register cleanup.
afterEach(() => cleanup());
