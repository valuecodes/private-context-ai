// Applies the saved Appearance choice before first paint, so the app never
// flashes the wrong theme. The `theme-init` plugin in `vite.config.ts` serves
// and emits this file as `/theme-init.js`, a classic same-origin script loaded
// from <head>: the CSP allows it (`script-src 'self'`) and it runs before any
// stylesheet or the body. It has no imports or exports, so it compiles to a
// plain script. Keep the key and rules in step with `src/lib/theme.ts`.
{
  let theme: "system" | "light" | "dark" = "system";
  try {
    const stored = globalThis.localStorage.getItem("theme");
    if (stored === "light" || stored === "dark") {
      theme = stored;
    }
  } catch {
    // Storage unavailable: follow the OS.
  }
  if (theme === "system") {
    theme = globalThis.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  }
  document.documentElement.dataset.theme = theme;
}
