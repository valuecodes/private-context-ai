import { describe, expect, it, vi } from "vitest";

import type { Conversation } from "~/lib/types";

import type { ConversationsState } from "./use-conversations";
import {
  conversationsReducer,
  initialConversations,
} from "./use-conversations";

vi.mock("~/lib/ipc");

const conversation = (id: string, title = id): Conversation => ({
  id,
  title,
  model: "llama3.2",
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
  settings: { systemPrompt: "", options: {}, think: { kind: "off" } },
});

const loaded = (...ids: string[]): ConversationsState =>
  conversationsReducer(initialConversations, {
    type: "loaded",
    conversations: ids.map((id) => conversation(id)),
  });

const ids = (state: ConversationsState): string[] =>
  state.conversations.map(({ id }) => id);

describe("conversationsReducer", () => {
  it("starts loading with nothing selected", () => {
    expect(initialConversations.phase).toBe("loading");
    expect(initialConversations.selectedId).toBeNull();
  });

  it("shows the loaded list", () => {
    const state = loaded("a", "b");
    expect(state.phase).toBe("ready");
    expect(ids(state)).toEqual(["a", "b"]);
  });

  it("keeps a chat created while the list was loading", () => {
    let state = conversationsReducer(initialConversations, {
      type: "created",
      conversation: conversation("new"),
    });
    state = conversationsReducer(state, {
      type: "loaded",
      conversations: [conversation("old")],
    });
    expect(ids(state)).toEqual(["new", "old"]);
    expect(state.selectedId).toBe("new");

    const again = conversationsReducer(state, {
      type: "loaded",
      conversations: [conversation("new"), conversation("old")],
    });
    expect(ids(again)).toEqual(["new", "old"]);
  });

  it("puts a created conversation first and selects it", () => {
    const state = conversationsReducer(loaded("a"), {
      type: "created",
      conversation: conversation("new"),
    });
    expect(ids(state)).toEqual(["new", "a"]);
    expect(state.selectedId).toBe("new");
  });

  it("replaces a renamed conversation in place", () => {
    const state = conversationsReducer(loaded("a", "b"), {
      type: "renamed",
      conversation: conversation("b", "Better"),
    });
    expect(state.conversations.map(({ title }) => title)).toEqual([
      "a",
      "Better",
    ]);
  });

  it("clears the selection when the open conversation is deleted", () => {
    let state = conversationsReducer(loaded("a", "b"), {
      type: "selected",
      id: "a",
    });
    state = conversationsReducer(state, { type: "deleted", id: "b" });
    expect(state.selectedId).toBe("a");
    state = conversationsReducer(state, { type: "deleted", id: "a" });
    expect(state.selectedId).toBeNull();
    expect(ids(state)).toEqual([]);
  });

  it("moves a used conversation to the top with the model it used", () => {
    const state = conversationsReducer(loaded("a", "b", "c"), {
      type: "touched",
      id: "c",
      model: "qwen3",
    });
    expect(ids(state)).toEqual(["c", "a", "b"]);
    expect(state.conversations[0]?.model).toBe("qwen3");
    expect(state.conversations[1]?.model).toBe("llama3.2");
    expect(
      conversationsReducer(state, {
        type: "touched",
        id: "missing",
        model: "x",
      })
    ).toEqual(state);
  });

  it("keeps the list on a failure and clears the error on success", () => {
    const error = { kind: "invalidInput", message: "bad title" } as const;
    let state = conversationsReducer(loaded("a"), { type: "failed", error });
    expect(state.error).toEqual(error);
    expect(ids(state)).toEqual(["a"]);
    state = conversationsReducer(state, { type: "selected", id: "a" });
    expect(state.error).toBeNull();
  });
});
