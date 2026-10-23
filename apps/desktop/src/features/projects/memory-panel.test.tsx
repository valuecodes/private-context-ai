import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { MemoryEntry } from "~/lib/types";

import { entryNotes, MemoryList } from "./memory-panel";
import type { MemoryState } from "./use-project-memory";
import { initialMemory } from "./use-project-memory";

vi.mock("~/lib/ipc");

const entry = (overrides: Partial<MemoryEntry> = {}): MemoryEntry => ({
  conversationId: "a",
  title: "Taxes",
  content: "Deadline is in May.",
  model: "qwen3",
  updatedAt: "2026-10-01T10:00:00.000Z",
  userEdited: false,
  rebuilt: false,
  truncated: false,
  state: "current",
  ...overrides,
});

const list = (
  state: Partial<MemoryState>,
  { autoMemory = true, disabled = false } = {}
) =>
  renderToString(
    <MemoryList
      state={{ ...initialMemory, phase: "ready", ...state }}
      autoMemory={autoMemory}
      disabled={disabled}
      titleOf={(id) => `Chat ${id}`}
      onRefresh={vi.fn()}
      onUpdate={() => Promise.resolve(true)}
      onRemove={vi.fn()}
      onAutoMemory={vi.fn()}
      onPreview={vi.fn()}
    />
  );

describe("entryNotes", () => {
  it("says nothing for a current entry", () => {
    expect(entryNotes(entry())).toEqual([]);
  });

  it("explains stale, pending, edited, rebuilt and shortened entries", () => {
    expect(entryNotes(entry({ state: "stale" }))[0]).toMatch(/Out of date/);
    expect(entryNotes(entry({ state: "pending" }))).toEqual([
      "New messages not in memory yet",
    ]);
    expect(
      entryNotes(entry({ rebuilt: true, userEdited: true, truncated: true }))
    ).toEqual([
      "Rebuilt after the chat was edited",
      "Edited",
      "A long message was shortened",
    ]);
  });
});

describe("MemoryList", () => {
  it("lists entries with their title, model and controls", () => {
    const html = list({ entries: [entry()] });
    expect(html).toContain("Taxes");
    expect(html).toContain("Deadline is in May.");
    expect(html).toContain("by qwen3");
    expect(html).toContain('aria-label="Refresh memory of Taxes"');
    expect(html).toContain('aria-label="Edit memory of Taxes"');
    expect(html).toContain('aria-label="Delete memory of Taxes"');
    expect(html).toContain("Refresh memory");
    expect(html).toContain("View full memory");
  });

  it("says when nothing is remembered yet", () => {
    expect(list({ entries: [] })).toContain("Nothing remembered yet");
    expect(list({ phase: "loading" })).not.toContain("Nothing remembered");
  });

  it("shows the Automatic memory switch as saved", () => {
    expect(list({})).toContain('checked=""');
    expect(list({}, { autoMemory: false })).not.toContain('checked=""');
  });

  it("disables refreshing while a refresh or a reply runs", () => {
    expect(list({ refreshing: "all", entries: [entry()] })).toContain(
      "Refreshing…"
    );
    const html = list({ entries: [entry()] }, { disabled: true });
    expect(html).toMatch(/disabled=""[^>]*>.*Refresh memory/);
  });

  it("names the chats a refresh could not summarize", () => {
    const html = list({
      failures: [
        {
          conversationId: "b",
          outcome: "failed",
          error: { kind: "ollama", message: "the summary was too long" },
        },
      ],
    });
    expect(html).toContain('role="alert"');
    expect(html).toContain("Chat b: the summary was too long");
  });
});
