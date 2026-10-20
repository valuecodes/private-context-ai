import { describe, expect, it, vi } from "vitest";

import type { ChatEvent, StoredMessage } from "~/lib/types";

import type { ChatState } from "./use-chat-stream";
import {
  chatReducer,
  createChatState,
  fromStored,
  titleFromPrompt,
} from "./use-chat-stream";

vi.mock("~/lib/ipc");

const sent = (text = "Hi", streamId = "s1"): ChatState =>
  chatReducer(createChatState("c1"), { type: "send", streamId, text });

const stored = (
  id: string,
  role: StoredMessage["role"],
  status: StoredMessage["status"],
  content = id
): StoredMessage => ({
  id,
  role,
  content,
  status,
  createdAt: "2026-01-01T00:00:00.000Z",
  promptTokens: null,
  completionTokens: null,
  durationMs: null,
});

const event = (
  state: ChatState,
  chatEvent: ChatEvent,
  streamId = "s1"
): ChatState =>
  chatReducer(state, { type: "event", streamId, event: chatEvent });

const done = (doneReason: string | null = "stop"): ChatEvent => ({
  type: "done",
  promptTokens: 1,
  completionTokens: 2,
  durationMs: 3,
  doneReason,
});

describe("chatReducer", () => {
  it("appends the user turn and a streaming assistant bubble on send", () => {
    const state = sent();
    expect(state.phase).toBe("streaming");
    expect(state.streamId).toBe("s1");
    expect(state.messages).toEqual([
      { id: "s1:user", role: "user", content: "Hi", status: "complete" },
      {
        id: "s1:assistant",
        role: "assistant",
        content: "",
        status: "streaming",
      },
    ]);
  });

  it("appends tokens to the assistant bubble", () => {
    let state = event(sent(), { type: "token", content: "Hel" });
    state = event(state, { type: "token", content: "lo" });
    expect(state.messages.at(-1)?.content).toBe("Hello");
    expect(state.messages.at(-1)?.status).toBe("streaming");
  });

  it("completes the reply on done", () => {
    const state = event(
      event(sent(), { type: "token", content: "Hey" }),
      done()
    );
    expect(state.phase).toBe("idle");
    expect(state.streamId).toBeNull();
    expect(state.notice).toBeNull();
    expect(state.messages.at(-1)?.status).toBe("complete");
  });

  it("shows a hint when the reply hit the length limit", () => {
    const state = event(sent(), done("length"));
    expect(state.notice).toBe("length");
    expect(state.messages.at(-1)?.status).toBe("complete");
  });

  it("marks the bubble failed on an error event", () => {
    const error = {
      kind: "timeout",
      message: "Ollama stopped responding",
    } as const;
    const state = event(sent(), { type: "error", error });
    expect(state.phase).toBe("idle");
    expect(state.error).toEqual(error);
    expect(state.messages.at(-1)?.status).toBe("error");
  });

  it("marks the bubble cancelled on cancel and returns to idle", () => {
    let state = event(sent(), { type: "token", content: "Par" });
    state = chatReducer(state, { type: "cancel" });
    expect(state.phase).toBe("idle");
    expect(state.messages.at(-1)).toMatchObject({
      content: "Par",
      status: "cancelled",
    });
  });

  it("shows busy and appends nothing on a send while streaming", () => {
    const before = sent();
    const state = chatReducer(before, {
      type: "send",
      streamId: "s2",
      text: "Again",
    });
    expect(state.notice).toBe("busy");
    expect(state.messages).toEqual(before.messages);
    expect(state.streamId).toBe("s1");
  });

  it("drops the bubble and fails the prompt when Rust refuses as busy", () => {
    const state = chatReducer(sent(), {
      type: "failed",
      streamId: "s1",
      error: { kind: "busy", message: "still answering" },
    });
    expect(state.phase).toBe("idle");
    expect(state.notice).toBe("busy");
    expect(state.error).toBeNull();
    expect(state.messages).toEqual([
      { id: "s1:user", role: "user", content: "Hi", status: "error" },
    ]);
  });

  it("keeps a non-busy refusal as the error", () => {
    const error = {
      kind: "invalidBaseUrl",
      message: "host must be loopback",
    } as const;
    const state = chatReducer(sent(), {
      type: "failed",
      streamId: "s1",
      error,
    });
    expect(state.error).toEqual(error);
    expect(state.notice).toBeNull();
  });

  it("ignores events and results from a stale stream", () => {
    let state = chatReducer(event(sent(), { type: "token", content: "A" }), {
      type: "cancel",
    });
    state = chatReducer(state, { type: "send", streamId: "s2", text: "Next" });
    const before = state;

    state = event(state, { type: "token", content: "late" }, "s1");
    state = event(state, done(), "s1");
    state = chatReducer(state, {
      type: "started",
      streamId: "s1",
      requestId: "r1",
    });
    state = chatReducer(state, {
      type: "failed",
      streamId: "s1",
      error: { kind: "busy", message: "x" },
    });

    expect(state).toEqual(before);
  });

  it("records the request id of the current stream", () => {
    const state = chatReducer(sent(), {
      type: "started",
      streamId: "s1",
      requestId: "r1",
    });
    expect(state.requestId).toBe("r1");
  });
});

describe("opening conversations", () => {
  it("clears the view and loads the saved messages", () => {
    let state = chatReducer(sent(), { type: "cancel" });
    state = chatReducer(state, { type: "open", conversationId: "c2" });
    expect(state).toEqual({ ...createChatState("c2"), loading: true });

    state = chatReducer(state, {
      type: "loaded",
      conversationId: "c2",
      messages: fromStored([stored("u", "user", "complete")]),
    });
    expect(state.loading).toBe(false);
    expect(state.messages).toEqual([
      { id: "u", role: "user", content: "u", status: "complete" },
    ]);
  });

  it("refuses a send until the saved messages have loaded", () => {
    const loading = chatReducer(createChatState(), {
      type: "open",
      conversationId: "c2",
    });
    expect(
      chatReducer(loading, { type: "send", streamId: "s1", text: "Hi" })
    ).toBe(loading);

    expect(
      chatReducer(createChatState(), { type: "open", conversationId: null })
        .loading
    ).toBe(false);
  });

  it("clears the view only if the deleted conversation is still open", () => {
    const open = chatReducer(createChatState("a"), {
      type: "deleted",
      conversationId: "b",
    });
    expect(open.conversationId).toBe("a");
    expect(chatReducer(open, { type: "deleted", conversationId: "a" })).toEqual(
      createChatState()
    );
  });

  it("ignores a load for a conversation that is no longer open", () => {
    let state = chatReducer(createChatState(), {
      type: "open",
      conversationId: "a",
    });
    state = chatReducer(state, { type: "open", conversationId: "b" });
    const before = state;

    state = chatReducer(state, {
      type: "loaded",
      conversationId: "a",
      messages: fromStored([stored("x", "user", "complete")]),
    });
    state = chatReducer(state, {
      type: "loadFailed",
      conversationId: "a",
      error: { kind: "notFound", message: "gone" },
    });
    expect(state).toEqual(before);
  });

  it("shows a failed load as the error", () => {
    const error = { kind: "database", message: "locked" } as const;
    const loading = chatReducer(createChatState(), {
      type: "open",
      conversationId: "a",
    });
    const state = chatReducer(loading, {
      type: "loadFailed",
      conversationId: "a",
      error,
    });
    expect(state.error).toEqual(error);
    expect(state.loading).toBe(false);
  });

  it("does not switch away from a streaming reply", () => {
    const before = sent();
    expect(chatReducer(before, { type: "open", conversationId: "c2" })).toBe(
      before
    );
  });

  it("adopts the conversation a first send created", () => {
    let state = chatReducer(createChatState(), {
      type: "send",
      streamId: "s1",
      text: "Hi",
    });
    state = chatReducer(state, {
      type: "created",
      streamId: "s1",
      conversationId: "new",
    });
    expect(state.conversationId).toBe("new");
    expect(state.messages).toHaveLength(2);

    const stale = chatReducer(state, {
      type: "created",
      streamId: "old",
      conversationId: "other",
    });
    expect(stale.conversationId).toBe("new");
  });

  it("adopts the created conversation after an early stop", () => {
    let state = chatReducer(createChatState(), {
      type: "send",
      streamId: "s1",
      text: "Hi",
    });
    state = chatReducer(state, { type: "cancel" });
    state = chatReducer(state, {
      type: "created",
      streamId: "s1",
      conversationId: "new",
    });
    expect(state.conversationId).toBe("new");
  });
});

describe("fromStored", () => {
  it("maps statuses and drops system messages", () => {
    expect(
      fromStored([
        stored("s", "system", "complete"),
        stored("u", "user", "complete"),
        stored("p", "assistant", "pending"),
        stored("c", "assistant", "cancelled"),
        stored("e", "assistant", "error"),
      ]).map(({ id, status }) => [id, status])
    ).toEqual([
      ["u", "complete"],
      ["p", "streaming"],
      ["c", "cancelled"],
      ["e", "error"],
    ]);
  });
});

describe("titleFromPrompt", () => {
  it("uses the first non-empty line", () => {
    expect(titleFromPrompt("\n  \n  Plan the trip  \nmore")).toBe(
      "Plan the trip"
    );
  });

  it("cuts long lines to 60 characters", () => {
    expect(titleFromPrompt("x".repeat(100))).toBe("x".repeat(60));
    expect([...(titleFromPrompt("😀".repeat(70)) ?? "")]).toHaveLength(60);
  });

  it("replaces control characters", () => {
    expect(titleFromPrompt("a\tb")).toBe("a b");
  });

  it("leaves a blank prompt to the default", () => {
    expect(titleFromPrompt(" \n\t\n")).toBeNull();
  });
});
