import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { applyTheme } from "~/lib/theme";

import { App } from "./app";

// Bundled fonts, so text looks the same on every OS. Imported here, not from
// `globals.css`: Tailwind inlines CSS imports, which breaks their relative
// `url()`s. Vite copies the `.woff2` files into `dist/assets` (too large to
// inline), so they load from 'self' under the CSP; `unicode-range` loads only
// the subsets a page uses.
import "@fontsource-variable/inter/wght.css";
import "@fontsource-variable/jetbrains-mono/wght.css";
import "./globals.css";

// `theme-init.ts` already set the theme; this keeps it following the OS.
applyTheme();

// Tauri's own drag and drop is off (`dragDropEnabled`), so the project files
// panel receives HTML5 drops. A file dropped anywhere else would otherwise
// be opened by the webview in place of the app.
for (const type of ["dragover", "drop"]) {
  globalThis.addEventListener(type, (event) => {
    event.preventDefault();
  });
}

const rootElement = document.getElementById("root");
if (rootElement) {
  const root = createRoot(rootElement);
  root.render(
    <StrictMode>
      <App />
    </StrictMode>
  );
} else {
  const message = "Root element with id 'root' not found";
  console.error(message);
  throw new Error(message);
}
