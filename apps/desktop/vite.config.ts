import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vite.dev/config/
// Port 1420 with `strictPort` because `tauri.conf.json` points `devUrl` (and
// the dev CSP's WebSocket entry) at exactly this port; a silent move to 1421
// would load nothing.
export default defineConfig({
  plugins: [react()],
  resolve: { tsconfigPaths: true },
  // Keep Rust compiler output visible when `tauri dev` runs Vite.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Cargo writes thousands of files under `src-tauri/target`.
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // The CSP's `font-src 'self'` refuses `data:` fonts, and some bundled
    // font subsets are under the 4 KB default, so fonts are always files.
    assetsInlineLimit: (file) => (file.endsWith(".woff2") ? false : undefined),
  },
});
