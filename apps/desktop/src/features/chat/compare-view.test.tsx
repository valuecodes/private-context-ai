import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { CompareState, UiMessage } from "./chat-reducer";
import { CompareView } from "./compare-view";

const candidate = (
  model: string,
  status: UiMessage["status"],
  content = ""
): UiMessage => ({ id: model, role: "assistant", content, status, model });

const comparison = (candidates: UiMessage[]): CompareState => ({
  group: "g",
  prompt: {
    id: "p",
    role: "user",
    content: "Which is better?",
    status: "complete",
  },
  candidates,
});

const render = (candidates: UiMessage[], comparing: boolean): string =>
  renderToString(
    <CompareView
      compare={comparison(candidates)}
      comparing={comparing}
      onPick={() => undefined}
      onDiscard={() => undefined}
    />
  );

const count = (html: string, text: string): number =>
  html.split(text).length - 1;

describe("CompareView", () => {
  it("shows one labelled column per model and which one is answering", () => {
    const html = render(
      [
        candidate("small", "complete", "First"),
        candidate("large", "streaming", "Sec"),
        candidate("other", "streaming"),
      ],
      true
    );
    expect(html).toContain("Which is better?");
    expect(count(html, "<li")).toBe(3);
    for (const model of ["small", "large", "other"]) {
      expect(html).toContain(`>${model}</span>`);
    }
    expect(html).toContain("Answering with large · 1 left");
    expect(html).toContain("Waiting");
    expect(html).toContain("sm:grid-cols-3");
    // Nothing can be kept or discarded while the models answer.
    expect(html).toMatch(/disabled=""[^>]*>Continue with this/);
    expect(html).toMatch(/disabled=""[^>]*>Discard/);
  });

  it("offers Continue only on complete answers once the run has ended", () => {
    const html = render(
      [
        candidate("small", "complete", "First"),
        candidate("large", "cancelled", "Half"),
        candidate("other", "error"),
      ],
      false
    );
    expect(count(html, "Continue with this")).toBe(1);
    expect(html).not.toMatch(/disabled=""[^>]*>Continue with this/);
    expect(html).toContain("Stopped");
    expect(html).toContain("Failed");
    expect(html).not.toMatch(/disabled=""[^>]*>Discard/);
  });
});
