import { describe, expect, it, vi } from "vitest";

import type { ChatEvent, StoredMessage } from "~/lib/types";

import type { ChatState } from "./chat-reducer";
import { chatReducer, createChatState, fromStored } from "./chat-reducer";

vi.mock("~/lib/ipc");

const stored = (
  id: string,
  role: StoredMessage["role"],
  content = id
): StoredMessage => ({
  id,
  role,
  content,
  status: "complete",
  createdAt: "2026-01-01T00:00:00.000Z",
  promptTokens: null,
  completionTokens: null,
  durationMs: null,
  thinking: null,
  thinkingMs: null,
});

const event = (state: ChatState, chatEvent: ChatEvent): ChatState =>
  chatReducer(state, {
    type: "event",
    streamId: "s1",
    event: chatEvent,
    at: 0,
  });

const done: ChatEvent = {
  type: "done",
  promptTokens: 1,
  completionTokens: 2,
  durationMs: 3,
  doneReason: "stop",
  thinkingMs: null,
};

/** Rust admitted stream `s1` as request `r1` with rows `n1` and `n2`. */
const started = (state: ChatState): ChatState =>
  chatReducer(state, {
    type: "started",
    streamId: "s1",
    started: { requestId: "r1", userMessageId: "n1", assistantMessageId: "n2" },
  });

const sent = (): ChatState =>
  chatReducer(createChatState("c1"), {
    type: "send",
    streamId: "s1",
    text: "Hi",
  });

/** A conversation of `One` and `Two`, each answered, as loaded. */
const twoTurns = (): ChatState =>
  chatReducer(
    chatReducer(createChatState(), { type: "open", conversationId: "c1" }),
    {
      type: "loaded",
      conversationId: "c1",
      messages: fromStored([
        stored("u1", "user", "One"),
        stored("a1", "assistant", "A1"),
        stored("u2", "user", "Two"),
        stored("a2", "assistant", "A2"),
      ]),
    }
  );

const regenerated = (state = twoTurns()): ChatState =>
  chatReducer(state, { type: "regenerate", streamId: "s1" });

const edited = (state = twoTurns()): ChatState =>
  chatReducer(state, {
    type: "edit",
    streamId: "s1",
    messageId: "u1",
    text: "Changed",
  });

const ids = (state: ChatState): string[] => state.messages.map(({ id }) => id);

const contents = (state: ChatState): string[] =>
  state.messages.map(({ content }) => content);

describe("Rust's message ids", () => {
  it("replace the local ids, and the reply keeps streaming", () => {
    let state = event(sent(), { type: "token", content: "Hel" });
    expect(state.messages[0]?.local).toBe(true);
    state = started(state);
    expect(ids(state)).toEqual(["n1", "n2"]);
    expect(state.messages[0]?.local).toBeUndefined();
    expect(state.requestId).toBe("r1");
    state = event(state, { type: "token", content: "lo" });
    state = event(state, done);
    expect(state.messages.at(-1)).toMatchObject({
      id: "n2",
      content: "Hello",
      status: "complete",
    });
  });

  it("replace them even after an early stop, without the request id", () => {
    const state = started(chatReducer(sent(), { type: "cancel" }));
    expect(ids(state)).toEqual(["n1", "n2"]);
    expect(state.requestId).toBeNull();
  });
});

describe("regenerate and edit", () => {
  it("replaces the last reply with a streaming one", () => {
    const state = regenerated();
    expect(state.phase).toBe("streaming");
    expect(contents(state)).toEqual(["One", "A1", "Two", ""]);
    expect(state.messages.at(-1)).toMatchObject({
      id: "s1:assistant",
      status: "streaming",
    });
  });

  it("replaces a user message and everything after it", () => {
    const state = edited();
    expect(contents(state)).toEqual(["Changed", ""]);
    expect(ids(state)).toEqual(["s1:user", "s1:assistant"]);
  });

  it("completes into the new messages", () => {
    let state = event(started(edited()), { type: "token", content: "New" });
    state = event(state, done);
    expect(contents(state)).toEqual(["Changed", "New"]);
    expect(ids(state)).toEqual(["n1", "n2"]);
    expect(state.revision).toBeNull();
  });

  it("refuses while a reply streams, and changes nothing", () => {
    const streaming = chatReducer(twoTurns(), {
      type: "send",
      streamId: "s0",
      text: "Three",
    });
    for (const state of [regenerated(streaming), edited(streaming)]) {
      expect(state.notice).toBe("busy");
      expect(state.messages).toEqual(streaming.messages);
      expect(state.streamId).toBe("s0");
    }
  });

  it("ignores a regenerate without a reply and an edit of a reply", () => {
    const loaded = twoTurns();
    const asked = chatReducer(loaded, {
      type: "send",
      streamId: "s0",
      text: "Three",
    });
    const failed = chatReducer(asked, {
      type: "failed",
      streamId: "s0",
      error: { kind: "unreachable", message: "down" },
    });
    expect(regenerated(failed)).toBe(failed);
    const reply = chatReducer(loaded, {
      type: "edit",
      streamId: "s1",
      messageId: "a1",
      text: "x",
    });
    expect(reply).toBe(loaded);
  });

  it("restores the conversation on every refusal", () => {
    const before = twoTurns().messages;
    const error = { kind: "unreachable", message: "Ollama is down" } as const;
    for (const start of [regenerated, edited]) {
      const rejected = chatReducer(start(), {
        type: "failed",
        streamId: "s1",
        error,
      });
      expect(rejected.messages).toEqual(before);
      expect(rejected.error).toEqual(error);
      expect(rejected.phase).toBe("idle");

      const failed = event(started(start()), { type: "error", error });
      expect(failed.messages).toEqual(before);
      expect(failed.error).toEqual(error);

      const stopped = chatReducer(start(), { type: "cancel" });
      expect(stopped.messages).toEqual(before);
      const stoppedLate = chatReducer(
        event(start(), { type: "token", content: "Half" }),
        { type: "cancel" }
      );
      expect(stoppedLate.messages).toEqual(before);
    }
  });

  it("still keeps a plain send's partial reply on stop", () => {
    const state = chatReducer(
      event(sent(), { type: "token", content: "Par" }),
      { type: "cancel" }
    );
    expect(contents(state)).toEqual(["Hi", "Par"]);
  });

  it("applies a reload only for the revision that ended last", () => {
    const messages = fromStored([stored("x", "user")]);
    const reload = (conversationId = "c1", streamId = "s1") =>
      ({ type: "reloaded", conversationId, streamId, messages }) as const;
    const stopped = chatReducer(regenerated(), { type: "cancel" });

    expect(chatReducer(stopped, reload()).messages).toEqual(messages);
    expect(chatReducer(stopped, reload("c2"))).toBe(stopped);
    expect(chatReducer(stopped, reload("c1", "s0"))).toBe(stopped);
    const streaming = regenerated();
    expect(chatReducer(streaming, reload())).toBe(streaming);
    // A later turn has ended since: the older reload is stale.
    const later = event(
      chatReducer(stopped, { type: "send", streamId: "s2", text: "Next" }),
      { type: "token", content: "x" }
    );
    const finished = chatReducer(later, {
      type: "event",
      streamId: "s2",
      event: done,
      at: 0,
    });
    expect(chatReducer(finished, reload())).toBe(finished);
  });
});
