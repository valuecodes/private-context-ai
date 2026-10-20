import { describe, expect, it, vi } from "vitest";

import type { ChatEvent } from "~/lib/types";

import type { ChatState } from "./use-chat-stream";
import {
  chatReducer,
  createChatState,
  historyForRequest,
} from "./use-chat-stream";

vi.mock("~/lib/ipc");

const sent = (text = "Hi", streamId = "s1"): ChatState =>
  chatReducer(createChatState("c1"), { type: "send", streamId, text });

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

describe("historyForRequest", () => {
  it("keeps complete turns and the visible part of a stopped reply", () => {
    let state = event(
      event(sent("One"), { type: "token", content: "Reply" }),
      done()
    );
    state = chatReducer(state, { type: "send", streamId: "s2", text: "Two" });
    state = event(state, { type: "token", content: "Half" }, "s2");
    state = chatReducer(state, { type: "cancel" });

    expect(historyForRequest(state.messages)).toEqual([
      { role: "user", content: "One" },
      { role: "assistant", content: "Reply" },
      { role: "user", content: "Two" },
      { role: "assistant", content: "Half" },
    ]);
  });

  it("leaves out empty stopped bubbles and failed turns", () => {
    let state = chatReducer(sent("Lost"), { type: "cancel" });
    state = chatReducer(state, {
      type: "send",
      streamId: "s2",
      text: "Refused",
    });
    state = chatReducer(state, {
      type: "failed",
      streamId: "s2",
      error: { kind: "busy", message: "x" },
    });
    state = chatReducer(state, {
      type: "send",
      streamId: "s3",
      text: "Errored",
    });
    state = event(state, { type: "token", content: "partial" }, "s3");
    state = event(
      state,
      { type: "error", error: { kind: "ollama", message: "boom" } },
      "s3"
    );

    expect(historyForRequest(state.messages)).toEqual([
      { role: "user", content: "Lost" },
      { role: "user", content: "Errored" },
    ]);
  });
});
