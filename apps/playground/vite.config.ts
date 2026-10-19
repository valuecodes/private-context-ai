import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv, type Plugin } from "vite";

/**
 * Cloudflare Web Analytics, only in a build given `VITE_CF_BEACON_TOKEN` (a
 * Workers Builds build variable). The token is public, it ships in the page, but
 * keeping it out of the repository keeps forks, previews and local builds from
 * reporting into the production site. Without it the page has no beacon.
 */
const cloudflareBeacon = (token: string): Plugin => ({
  name: "cloudflare-beacon",
  apply: "build",
  transformIndexHtml: () => {
    if (token === "") {
      return [];
    }
    if (!/^[0-9a-f]{32}$/.test(token)) {
      throw new Error("VITE_CF_BEACON_TOKEN must be 32 lowercase hex digits");
    }
    // A classic deferred script, as in Cloudflare's snippet: the beacon reads
    // `data-cf-beacon` from `document.currentScript`, which is null in a module.
    return [
      {
        tag: "script",
        attrs: {
          defer: true,
          src: "https://static.cloudflareinsights.com/beacon.min.js",
          "data-cf-beacon": JSON.stringify({ token }),
        },
        injectTo: "head",
      },
    ];
  },
});

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [
    react(),
    cloudflareBeacon(
      loadEnv(mode, process.cwd(), "VITE_").VITE_CF_BEACON_TOKEN ?? ""
    ),
  ],
  resolve: { tsconfigPaths: true },
}));
