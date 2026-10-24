import react from "@vitejs/plugin-react";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { defineConfig, transformWithOxc } from "vite";
import type { Plugin } from "vite";

const THEME_INIT_SOURCE = fileURLToPath(
  new URL("src/theme-init.ts", import.meta.url)
);

const compileThemeInit = async () => {
  const code = await readFile(THEME_INIT_SOURCE, "utf8");
  const result = await transformWithOxc(code, THEME_INIT_SOURCE, {
    lang: "ts",
  });
  return result.code;
};

// Serves and emits `src/theme-init.ts` as `/theme-init.js`, a classic
// synchronous script for <head>. It cannot be a Vite module entry, since
// module scripts are deferred and the wrong theme could paint first, and it
// cannot be inline, since the CSP allows only same-origin script files.
const themeInit = (): Plugin => ({
  name: "theme-init",
  configureServer: (server) => {
    server.middlewares.use("/theme-init.js", (_req, res, next) => {
      compileThemeInit().then((code) => {
        res.setHeader("Content-Type", "text/javascript");
        res.end(code);
      }, next);
    });
  },
  async generateBundle() {
    this.emitFile({
      type: "asset",
      fileName: "theme-init.js",
      source: await compileThemeInit(),
    });
  },
});

// https://vite.dev/config/
// Port 1420 with `strictPort` because `tauri.conf.json` points `devUrl` (and
// the dev CSP's WebSocket entry) at exactly this port; a silent move to 1421
// would load nothing.
export default defineConfig({
  plugins: [react(), themeInit()],
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
