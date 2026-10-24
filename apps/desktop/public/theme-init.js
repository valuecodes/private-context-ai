// Applies the saved Appearance choice before first paint, so the app never
// flashes the wrong theme. A classic same-origin script loaded from <head>:
// the CSP allows it (`script-src 'self'`) and it runs before any stylesheet
// or the body. Keep the key and rules in step with `src/lib/theme.ts`.
{
  let theme = "system";
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
