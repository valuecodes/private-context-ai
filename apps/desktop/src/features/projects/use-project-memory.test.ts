import { describe, expect, it, vi } from "vitest";

import type { MemoryEntry } from "~/lib/types";

import { initialMemory, memoryReducer } from "./use-project-memory";

vi.mock("~/lib/ipc");

const entry = (conversationId: string, content = "Notes"): MemoryEntry => ({
  conversationId,
  title: `Chat ${conversationId}`,
  content,
  model: "qwen3",
  updatedAt: "x",
  userEdited: false,
  rebuilt: false,
  truncated: false,
  state: "current",
});

const ids = (entries: MemoryEntry[]): string[] =>
  entries.map(({ conversationId }) => conversationId);

describe("memoryReducer", () => {
  it("starts loading with nothing", () => {
    expect(initialMemory.phase).toBe("loading");
    expect(initialMemory.entries).toEqual([]);
    expect(initialMemory.refreshing).toBeNull();
  });

  it("keeps Rust's order when loaded", () => {
    const state = memoryReducer(initialMemory, {
      type: "loaded",
      entries: [entry("b"), entry("a")],
    });
    expect(state.phase).toBe("ready");
    expect(ids(state.entries)).toEqual(["b", "a"]);
  });

  it("puts an edited entry first and drops a deleted one", () => {
    const loaded = memoryReducer(initialMemory, {
      type: "loaded",
      entries: [entry("b"), entry("a")],
    });
    const edited = memoryReducer(loaded, {
      type: "saved",
      entry: { ...entry("a", "Mine"), userEdited: true },
    });
    expect(ids(edited.entries)).toEqual(["a", "b"]);
    expect(edited.entries[0]?.content).toBe("Mine");

    const deleted = memoryReducer(edited, {
      type: "deleted",
      conversationId: "b",
    });
    expect(ids(deleted.entries)).toEqual(["a"]);
  });

  it("tracks a refresh and keeps only its failures", () => {
    const refreshing = memoryReducer(initialMemory, {
      type: "refreshing",
      target: "all",
    });
    expect(refreshing.refreshing).toBe("all");

    const done = memoryReducer(refreshing, {
      type: "refreshed",
      results: [
        { conversationId: "a", outcome: "updated", error: null },
        {
          conversationId: "b",
          outcome: "busy",
          error: { kind: "busy", message: "this chat is answering" },
        },
        { conversationId: "c", outcome: "current", error: null },
      ],
    });
    expect(done.refreshing).toBeNull();
    expect(done.failures.map(({ conversationId }) => conversationId)).toEqual([
      "b",
    ]);

    // The next refresh starts clean.
    expect(
      memoryReducer(done, { type: "refreshing", target: "a" }).failures
    ).toEqual([]);
  });

  it("shows a refused call's error and ends the refresh", () => {
    const state = memoryReducer(
      memoryReducer(initialMemory, { type: "refreshing", target: "a" }),
      { type: "failed", error: { kind: "busy", message: "busy" } }
    );
    expect(state.refreshing).toBeNull();
    expect(state.error?.message).toBe("busy");
    expect(state.phase).toBe("ready");
  });
});
