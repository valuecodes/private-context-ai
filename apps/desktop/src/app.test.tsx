import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { App } from "./app";

vi.mock("~/lib/ipc");

describe("App", () => {
  const html = renderToString(<App />);

  it("renders the conversation sidebar placeholder", () => {
    expect(html).toContain("<aside");
    expect(html).toContain("No conversations yet");
  });

  it("renders the empty chat area", () => {
    expect(html).toMatch(/<main[^>]*>/);
  });

  it("renders the Ollama status line", () => {
    expect(html).toMatch(/role="status"[^>]*>Ollama: checking…</);
  });

  it("renders the model picker and the composer", () => {
    expect(html).toMatch(/<select[^>]*id="model"/);
    expect(html).toMatch(/<textarea[^>]*id="composer"/);
    expect(html).toMatch(/<button[^>]*type="submit"[^>]*disabled/);
  });
});
