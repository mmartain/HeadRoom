import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { join } from "node:path";
import { homedir } from "node:os";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Keep Vite's prebundle cache off Dropbox — file locks there cause EBUSY white screens.
const localAppData =
  process.env.LOCALAPPDATA || join(homedir(), "AppData", "Local");
const viteCacheDir = join(localAppData, "headroom-vite");

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  cacheDir: viteCacheDir,

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
