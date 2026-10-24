import { describe, expect, it, vi } from "vitest";

import { resolveTheme, THEME_STORAGE_KEY } from "./theme";

type Listener = (event: { matches: boolean }) => void;

/** A fake OS colour scheme, `localStorage` and `<html>`. */
const stubBrowser = ({
  stored = null,
  prefersDark = false,
  storageThrows = false,
}: {
  stored?: string | null;
  prefersDark?: boolean;
  storageThrows?: boolean;
} = {}) => {
  vi.unstubAllGlobals();
  const items = new Map<string, string>();
  if (stored !== null) {
    items.set(THEME_STORAGE_KEY, stored);
  }
  const listeners: Listener[] = [];
  const os = { dark: prefersDark };
  const html = { dataset: {} as Record<string, string> };
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => {
      if (storageThrows) {
        throw new Error("denied");
      }
      return items.get(key) ?? null;
    },
    setItem: (key: string, value: string) => {
      if (storageThrows) {
        throw new Error("denied");
      }
      items.set(key, value);
    },
  });
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query === "(prefers-color-scheme: dark)" && os.dark,
    addEventListener: (_type: string, listener: Listener) => {
      listeners.push(listener);
    },
  }));
  vi.stubGlobal("document", { documentElement: html });
  return {
    items,
    theme: () => html.dataset.theme,
    /** Switches the OS scheme and fires `change`. */
    setOs: (dark: boolean) => {
      os.dark = dark;
      for (const listener of listeners) {
        listener({ matches: dark });
      }
    },
  };
};

// Each test gets a fresh module, so its session choice starts unread.
const loadTheme = async () => {
  vi.resetModules();
  return import("./theme");
};

// The pre-paint script, run as the webview would run it.
const runInitScript = async () => {
  vi.resetModules();
  const path = "/public/theme-init.js";
  await import(/* @vite-ignore */ path);
};

describe("resolveTheme", () => {
  it("follows the OS under System", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("lets Light and Dark override the OS", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
});

describe("getTheme", () => {
  it("reads the saved choice", async () => {
    stubBrowser({ stored: "dark" });
    const { getTheme } = await loadTheme();
    expect(getTheme()).toBe("dark");
  });

  it("falls back to System when storage throws", async () => {
    stubBrowser({ stored: "dark", storageThrows: true });
    const { getTheme } = await loadTheme();
    expect(getTheme()).toBe("system");
  });

  it("falls back to System for an unknown value", async () => {
    stubBrowser({ stored: "sepia" });
    const { getTheme } = await loadTheme();
    expect(getTheme()).toBe("system");
  });
});

describe("applyTheme", () => {
  it("follows a live OS change under System", async () => {
    const browser = stubBrowser({ prefersDark: false });
    const { applyTheme } = await loadTheme();
    applyTheme();
    expect(browser.theme()).toBe("light");
    browser.setOs(true);
    expect(browser.theme()).toBe("dark");
  });

  it("ignores an OS change under Light", async () => {
    const browser = stubBrowser({ stored: "light", prefersDark: false });
    const { applyTheme } = await loadTheme();
    applyTheme();
    browser.setOs(true);
    expect(browser.theme()).toBe("light");
  });
});

describe("setTheme", () => {
  it("persists and applies the choice", async () => {
    const browser = stubBrowser({ prefersDark: false });
    const { applyTheme, getTheme, setTheme } = await loadTheme();
    applyTheme();
    setTheme("dark");
    expect(browser.items.get(THEME_STORAGE_KEY)).toBe("dark");
    expect(browser.theme()).toBe("dark");
    expect(getTheme()).toBe("dark");
  });

  it("keeps an unsaved choice for the session, across OS changes", async () => {
    const browser = stubBrowser({ prefersDark: true, storageThrows: true });
    const { applyTheme, getTheme, setTheme } = await loadTheme();
    applyTheme();
    setTheme("light");
    expect(browser.theme()).toBe("light");
    browser.setOs(false);
    browser.setOs(true);
    expect(browser.theme()).toBe("light");
    expect(getTheme()).toBe("light");
  });
});

describe("theme-init.js", () => {
  it.each([
    { stored: "light", prefersDark: true, expected: "light" },
    { stored: "dark", prefersDark: false, expected: "dark" },
    { stored: null, prefersDark: true, expected: "dark" },
    { stored: null, prefersDark: false, expected: "light" },
    { stored: "system", prefersDark: true, expected: "dark" },
    { stored: "sepia", prefersDark: false, expected: "light" },
  ])(
    "sets $expected for a saved $stored on a dark=$prefersDark OS",
    async ({ stored, prefersDark, expected }) => {
      const browser = stubBrowser({ stored, prefersDark });
      await runInitScript();
      expect(browser.theme()).toBe(expected);

      // The app's module agrees with what was painted.
      const { applyTheme } = await loadTheme();
      applyTheme();
      expect(browser.theme()).toBe(expected);
    }
  );

  it("follows the OS when storage throws", async () => {
    const browser = stubBrowser({
      stored: "light",
      prefersDark: true,
      storageThrows: true,
    });
    await runInitScript();
    expect(browser.theme()).toBe("dark");
  });
});
