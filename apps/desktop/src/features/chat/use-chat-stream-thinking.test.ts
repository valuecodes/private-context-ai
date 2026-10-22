import { describe, expect, it, vi } from "vitest";

import type { ChatEvent, StoredMessage } from "~/lib/types";

import type { ChatState } from "./chat-reducer";
import { chatReducer, createChatState } from "./chat-reducer";
import { fromStored } from "./stored-messages";

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
  evalMs: null,
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
  evalMs: null,
});

const noStats = { completionTokens: null, durationMs: null, evalMs: null };

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

  it("leaves replies without thinking alone, keeping their stats", () => {
    let state = event(sent(), { type: "token", content: "Hi" }, 3000);
    state = event(state, done(null));
    expect(state.messages.at(-1)).toEqual({
      id: "s1:assistant",
      role: "assistant",
      content: "Hi",
      status: "complete",
      live: true,
      stats: { completionTokens: 2, durationMs: 3, evalMs: null },
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
        stats: noStats,
      },
      {
        id: "b",
        role: "assistant",
        content: "b",
        status: "cancelled",
        stats: noStats,
      },
      {
        id: "c",
        role: "assistant",
        content: "c",
        status: "complete",
        stats: noStats,
      },
    ]);
  });

  it("restores saved stats on replies only", () => {
    const user: StoredMessage = {
      ...stored("u", "complete", null),
      role: "user",
      completionTokens: 9,
    };
    const reply: StoredMessage = {
      ...stored("r", "complete", null),
      completionTokens: 100,
      durationMs: 2500,
      evalMs: 2000,
    };
    const [prompt, answer] = fromStored([user, reply]);
    expect(prompt?.stats).toBeUndefined();
    expect(answer?.stats).toEqual({
      completionTokens: 100,
      durationMs: 2500,
      evalMs: 2000,
    });
  });
});
