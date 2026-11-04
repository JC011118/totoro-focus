import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

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
    // Production pages only. dev.html (browser preview + mock backend) is never built.
    rollupOptions: {
      input: {
        companion: resolve(__dirname, "companion.html"),
        settings: resolve(__dirname, "settings.html"),
      },
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
