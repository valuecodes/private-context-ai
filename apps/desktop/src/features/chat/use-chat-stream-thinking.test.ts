import { describe, expect, it, vi } from "vitest";

import type { ChatEvent, StoredMessage } from "~/lib/types";

import type { ChatState } from "./use-chat-stream";
import { chatReducer, createChatState, fromStored } from "./use-chat-stream";

vi.mock("~/lib/ipc");

const sent = (): ChatState =>
  chatReducer(createChatState("c1"), {
    type: "send",
    streamId: "s1",
    text: "Hi",
  });

const event = (state: ChatState, chatEvent: ChatEvent, at = 0): ChatState =>
  chatReducer(state, { type: "event", streamId: "s1", event: chatEvent, at });

const thought = (state: ChatState, content: string, at: number): ChatState =>
  event(state, { type: "thinking", content }, at);

const done = (thinkingMs: number | null): ChatEvent => ({
  type: "done",
  promptTokens: 1,
  completionTokens: 2,
  durationMs: 3,
  doneReason: "stop",
  thinkingMs,
});

const stored = (
  id: string,
  status: StoredMessage["status"],
  thinking: string | null,
  thinkingMs: number | null = null
): StoredMessage => ({
  id,
  role: "assistant",
  content: id,
  status,
  createdAt: "2026-01-01T00:00:00.000Z",
  promptTokens: null,
  completionTokens: null,
  durationMs: null,
  thinking,
  thinkingMs,
});

describe("thinking in the chat reducer", () => {
  it("collects thinking and times it to the first answer token", () => {
    let state = thought(sent(), "Let me", 1000);
    state = thought(state, " see", 1500);
    expect(state.messages.at(-1)).toMatchObject({
      content: "",
      thinking: "Let me see",
      status: "streaming",
    });
    expect(state.messages.at(-1)?.thinkingMs).toBeUndefined();

    state = event(state, { type: "token", content: "Hi" }, 3000);
    expect(state.messages.at(-1)?.thinkingMs).toBe(2000);
    state = event(state, { type: "token", content: "!" }, 9000);
    expect(state.messages.at(-1)?.thinkingMs).toBe(2000);
  });

  it("takes Rust's thinking time from done", () => {
    let state = thought(sent(), "Hmm", 1000);
    state = event(state, { type: "token", content: "Hi" }, 3000);
    state = event(state, done(1800));
    expect(state.messages.at(-1)).toMatchObject({
      thinkingMs: 1800,
      status: "complete",
    });
  });

  it("leaves replies without thinking alone", () => {
    let state = event(sent(), { type: "token", content: "Hi" }, 3000);
    state = event(state, done(null));
    expect(state.messages.at(-1)).toEqual({
      id: "s1:assistant",
      role: "assistant",
      content: "Hi",
      status: "complete",
    });
  });

  it("restores saved thinking", () => {
    expect(
      fromStored([
        stored("a", "complete", "Reasoning", 2500),
        stored("b", "cancelled", ""),
        stored("c", "complete", null),
      ])
    ).toEqual([
      {
        id: "a",
        role: "assistant",
        content: "a",
        status: "complete",
        thinking: "Reasoning",
        thinkingMs: 2500,
      },
      { id: "b", role: "assistant", content: "b", status: "cancelled" },
      { id: "c", role: "assistant", content: "c", status: "complete" },
    ]);
  });
});
