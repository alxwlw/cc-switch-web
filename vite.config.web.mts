import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Web build for the cc-switch-web fork (Tailscale-only server mode).
// Mirrors the desktop renderer pipeline, but swaps every `@tauri-apps/api/*`
// import for a headless shim (see src/lib/web-shims/) so the same React
// console talks to the Axum HTTP API instead of the Tauri IPC bridge.
export default defineConfig({
  root: "src",
  plugins: [react()],
  base: "/",
  build: {
    outDir: "../dist-web",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, "src/index.html"),
        "tray-panel": path.resolve(__dirname, "src/tray-panel.html"),
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@tauri-apps/api/core": path.resolve(__dirname, "./src/lib/web-shims/core.ts"),
      "@tauri-apps/api/event": path.resolve(__dirname, "./src/lib/web-shims/event.ts"),
      "@tauri-apps/api/app": path.resolve(__dirname, "./src/lib/web-shims/app.ts"),
      "@tauri-apps/api/window": path.resolve(__dirname, "./src/lib/web-shims/window.ts"),
      "@tauri-apps/api/path": path.resolve(__dirname, "./src/lib/web-shims/path.ts"),
    },
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
});
