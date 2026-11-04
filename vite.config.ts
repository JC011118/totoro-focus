import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const page = (name: string) => fileURLToPath(new URL(name, import.meta.url));

// Tauri expects a fixed dev port and serves the built `dist/` in production.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**", "**/target/**", "**/crates/**"] },
  },
  build: {
    // WKWebView on macOS 11+.
    target: ["es2021", "safari14"],
    sourcemap: false,
    // Production pages only. dev.html (browser preview with the mock backend)
    // is never an input; scripts/assert-no-mock.mjs fails the build if it leaks.
    rolldownOptions: {
      input: {
        companion: page("companion.html"),
        settings: page("settings.html"),
      },
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test/setup.ts"],
  },
});
