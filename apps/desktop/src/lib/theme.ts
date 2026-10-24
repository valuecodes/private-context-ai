/**
 * The Appearance setting. Stored in `localStorage`, not the encrypted
 * settings: it is a non-sensitive UI preference that must apply before the
 * database unlocks. `public/theme-init.js` applies it before first paint with
 * the same key and rules; this module keeps it applied afterwards.
 */
type Theme = "system" | "light" | "dark";

const THEMES: readonly Theme[] = ["system", "light", "dark"];

const THEME_STORAGE_KEY = "theme";

const DARK_QUERY = "(prefers-color-scheme: dark)";

const isTheme = (value: unknown): value is Theme =>
  THEMES.some((theme) => theme === value);

/** The colours to show for a choice: System follows the OS. */
const resolveTheme = (theme: Theme, prefersDark: boolean): "light" | "dark" => {
  if (theme === "system") {
    return prefersDark ? "dark" : "light";
  }
  return theme;
};

// The session's choice. Kept here, not only in storage, so a failed write
// still holds until the app quits, also across OS theme changes.
let current: Theme | null = null;
let subscribed = false;

const readStored = (): Theme => {
  try {
    const stored = globalThis.localStorage.getItem(THEME_STORAGE_KEY);
    return isTheme(stored) ? stored : "system";
  } catch {
    return "system";
  }
};

const getTheme = (): Theme => {
  current ??= readStored();
  return current;
};

const apply = (prefersDark: boolean) => {
  document.documentElement.dataset.theme = resolveTheme(
    getTheme(),
    prefersDark
  );
};

/** Sets `data-theme` on `<html>` and follows live OS changes under System. */
const applyTheme = () => {
  const media = globalThis.matchMedia(DARK_QUERY);
  apply(media.matches);
  if (!subscribed) {
    subscribed = true;
    media.addEventListener("change", (event) => {
      apply(event.matches);
    });
  }
};

/** Applies `theme` at once and remembers it across restarts. */
const setTheme = (theme: Theme) => {
  current = theme;
  try {
    globalThis.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Storage unavailable: the choice lasts for this session only.
  }
  applyTheme();
};

export type { Theme };
export {
  applyTheme,
  getTheme,
  resolveTheme,
  setTheme,
  THEME_STORAGE_KEY,
  THEMES,
};
