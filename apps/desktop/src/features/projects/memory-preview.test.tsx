import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { ContextPreview } from "~/lib/types";

import { PreviewBody } from "./memory-preview";

vi.mock("~/lib/ipc");

const preview: ContextPreview = {
  text: "## Project instructions\n\nCite sources.",
  entries: [
    { conversationId: "a", title: "Taxes", state: "included" },
    { conversationId: "b", title: "Thesis", state: "dropped" },
    { conversationId: "c", title: "Old", state: "stale" },
  ],
  memoryBytes: 1200,
  budgetBytes: 4096,
  files: { kind: "none" },
};

const body = (props: Partial<Parameters<typeof PreviewBody>[0]> = {}) =>
  renderToString(
    <PreviewBody
      preview={preview}
      error={null}
      forChat={false}
      copied={null}
      onCopy={vi.fn()}
      {...props}
    />
  );

describe("PreviewBody", () => {
  it("shows the exact text in monospace", () => {
    const html = body();
    expect(html).toContain("<pre");
    expect(html).toContain("## Project instructions\n\nCite sources.");
  });

  it("greys out dropped and stale entries", () => {
    const html = body();
    expect(html).toContain("Sent");
    expect(html).toContain("Over the budget");
    expect(html).toContain("Out of date");
    expect(html.match(/line-through/g)).toHaveLength(2);
  });

  it("meters memory against the budget", () => {
    const html = body();
    expect(html).toMatch(/<meter[^>]*max="4096"[^>]*value="1200"/);
    expect(html).toContain("1.2 KB of 4.1 KB");
  });

  it("explains what a chat sees", () => {
    expect(body({ forChat: true })).toContain("Its own memory is never sent");
    expect(body()).toContain("Each chat leaves out its own entry");
  });

  it("says how the project's files are used", () => {
    expect(body()).not.toContain("project&#x27;s files");
    expect(body({ preview: { ...preview, files: { kind: "off" } } })).toContain(
      "Files is off for this chat"
    );
    expect(
      body({ preview: { ...preview, files: { kind: "perMessage" } } })
    ).toContain("chosen for each message");
  });

  it("meters the passages found for the draft", () => {
    const html = body({
      preview: {
        ...preview,
        files: {
          kind: "searched",
          included: 2,
          dropped: 1,
          bytes: 3000,
          budgetBytes: 4096,
        },
      },
    });
    expect(html).toMatch(/<meter[^>]*id="files-budget"[^>]*value="3000"/);
    expect(html).toContain("3.0 KB of 4.1 KB");
    expect(html).toContain("2 passages");
    expect(html).toContain("1 more are over the budget");
    expect(
      body({
        preview: {
          ...preview,
          files: {
            kind: "searched",
            included: 0,
            dropped: 0,
            bytes: 0,
            budgetBytes: 4096,
          },
        },
      })
    ).toContain("No passage");
  });

  it("shows loading, errors and an empty message", () => {
    expect(body({ preview: null })).toContain("Loading…");
    expect(
      body({ error: { kind: "notFound", message: "no such chat" } })
    ).toContain("no such chat");
    expect(body({ preview: { ...preview, text: "" } })).toContain(
      "(no system message)"
    );
  });
});
