import js from "@eslint/js";
import { defineConfig, globalIgnores } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

// Mechanical guards for CLAUDE.md rule 1 ("backend owns time") and the dev-mock
// fence (SPEC §10, frontend-ipc-petdex-issues #11, #22).

const DEV_IMPORT = {
  regex: "(^|/)dev(/|$)",
  message: "src/dev/** (mock backend, dev harness) may only be imported from src/dev.",
};
const TAURI_IMPORT = {
  regex: "^@tauri-apps/",
  message: "Only src/ipc/tauriBackend.ts talks to Tauri; use the Backend interface.",
};
const TIME_MESSAGE =
  "The renderer never owns timing. Render Snapshot fields (remainingLabel, remainingMs) instead. " +
  "Clock reads are allowed only in src/dev/**, src/character/useSpriteClock.ts and src/theme/localTime.ts.";

const timingRules = {
  "no-restricted-globals": [
    "error",
    { name: "setInterval", message: TIME_MESSAGE },
    { name: "requestAnimationFrame", message: TIME_MESSAGE },
  ],
  "no-restricted-properties": [
    "error",
    { object: "Date", property: "now", message: TIME_MESSAGE },
    { object: "performance", property: "now", message: TIME_MESSAGE },
    { object: "window", property: "setInterval", message: TIME_MESSAGE },
    { object: "globalThis", property: "setInterval", message: TIME_MESSAGE },
    { object: "self", property: "setInterval", message: TIME_MESSAGE },
    { object: "window", property: "requestAnimationFrame", message: TIME_MESSAGE },
    { object: "globalThis", property: "requestAnimationFrame", message: TIME_MESSAGE },
    { object: "self", property: "requestAnimationFrame", message: TIME_MESSAGE },
  ],
  "no-restricted-syntax": [
    "error",
    { selector: "NewExpression[callee.name='Date'][arguments.length=0]", message: TIME_MESSAGE },
    { selector: "CallExpression[callee.name='Date']", message: TIME_MESSAGE },
    // window.Date / globalThis.Date / self.Date: new X.Date(), X.Date(), X.Date.now
    {
      selector: "NewExpression[callee.type='MemberExpression'][callee.property.name='Date'][arguments.length=0]",
      message: TIME_MESSAGE,
    },
    { selector: "CallExpression[callee.type='MemberExpression'][callee.property.name='Date']", message: TIME_MESSAGE },
    {
      selector: "MemberExpression[property.name='now'][object.type='MemberExpression'][object.property.name='Date']",
      message: TIME_MESSAGE,
    },
    {
      selector: "MemberExpression[property.name='now'][object.type='MemberExpression'][object.property.name='performance']",
      message: TIME_MESSAGE,
    },
  ],
};
const timingOff = {
  "no-restricted-globals": "off",
  "no-restricted-properties": "off",
  "no-restricted-syntax": "off",
};

export default defineConfig(
  globalIgnores(["dist", "target", "src-tauri", "crates", "src/ipc/bindings", "node_modules", "packs", "packs-local"]),
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: { ecmaVersion: 2022, globals: globals.browser },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
    },
  },
  // Renderer: no dev imports, no Tauri outside the adapter, no clock reads.
  {
    files: ["src/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": ["error", { patterns: [DEV_IMPORT, TAURI_IMPORT] }],
      ...timingRules,
    },
  },
  {
    files: ["src/ipc/tauriBackend.ts"],
    rules: { "no-restricted-imports": ["error", { patterns: [DEV_IMPORT] }] },
  },
  {
    files: ["src/dev/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": ["error", { patterns: [TAURI_IMPORT] }],
      ...timingOff,
    },
  },
  {
    // Presentation-only clocks: sprite frame stepping and local-time palette triggers.
    files: ["src/character/useSpriteClock.ts", "src/theme/localTime.ts"],
    rules: timingOff,
  },
  // Node tooling.
  {
    files: ["vite.config.ts"],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["scripts/**/*.mjs", "src/dev/fixtures/**/*.mjs", "eslint.config.js"],
    extends: [js.configs.recommended],
    languageOptions: { ecmaVersion: 2022, sourceType: "module", globals: globals.node },
  },
);
