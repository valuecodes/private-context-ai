import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { App, Workspace } from "./app";

vi.mock("~/lib/ipc");

describe("App", () => {
  it("waits for secure storage before showing any chat UI", () => {
    const html = renderToString(<App />);
    expect(html).toContain("Opening secure storage…");
    expect(html).not.toContain("<aside");
    expect(html).not.toContain("composer");
  });
});

describe("Workspace", () => {
  const html = renderToString(<Workspace />);

  it("renders the conversation sidebar while it loads", () => {
    expect(html).toContain("<aside");
    expect(html).toMatch(/<button[^>]*>New chat<\/button>/);
    expect(html).toContain("Loading…");
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
