import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// Set by `tauri ios/android dev` to the host LAN IP so a physical device can
// reach the dev server (and HMR websocket). Unset for desktop dev.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: {
    host: host || false,
    port: 1420,
    strictPort: true,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});
