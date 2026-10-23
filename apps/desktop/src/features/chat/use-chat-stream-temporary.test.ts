import { describe, expect, it, vi } from "vitest";

import { chatReducer, createChatState } from "./chat-reducer";

vi.mock("~/lib/ipc");

const streamingTemporary = () =>
  chatReducer(chatReducer(createChatState(), { type: "openTemporary" }), {
    type: "send",
    streamId: "s1",
    text: "Hi",
  });

describe("temporary chats in the reducer", () => {
  it("opens an empty temporary chat", () => {
    const state = chatReducer(createChatState(), { type: "openTemporary" });
    expect(state.temporary).toBe(true);
    expect(state.conversationId).toBeNull();
    expect(state.messages).toEqual([]);
  });

  it("leaves a temporary chat even while it answers", () => {
    const state = chatReducer(streamingTemporary(), {
      type: "open",
      conversationId: null,
    });
    expect(state).toEqual(createChatState());
  });

  it("ignores events of the discarded reply", () => {
    const left = chatReducer(streamingTemporary(), {
      type: "open",
      conversationId: null,
    });
    const state = chatReducer(left, {
      type: "event",
      streamId: "s1",
      event: { type: "token", content: "late" },
      at: 0,
    });
    expect(state).toBe(left);
  });

  it("closes a chat of a deleted project, even a streaming temporary one", () => {
    const temporary = chatReducer(
      chatReducer(createChatState(), {
        type: "openTemporary",
        projectId: "p1",
      }),
      { type: "send", streamId: "s1", text: "Hi" }
    );
    expect(temporary.projectId).toBe("p1");
    expect(
      chatReducer(temporary, { type: "projectDeleted", projectId: "p1" })
    ).toEqual(createChatState());

    // Another project's chat, or one opened since, stays.
    const saved = chatReducer(createChatState(), {
      type: "open",
      conversationId: "c1",
      projectId: "p2",
    });
    expect(
      chatReducer(saved, { type: "projectDeleted", projectId: "p1" })
    ).toBe(saved);
  });

  it("still keeps a saved chat open while it answers", () => {
    const streaming = chatReducer(createChatState("c1"), {
      type: "send",
      streamId: "s1",
      text: "Hi",
    });
    expect(chatReducer(streaming, { type: "open", conversationId: null })).toBe(
      streaming
    );
    expect(chatReducer(streaming, { type: "openTemporary" })).toBe(streaming);
  });
});
