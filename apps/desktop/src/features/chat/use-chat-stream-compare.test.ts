import { describe, expect, it, vi } from "vitest";

import type {
  AppError,
  CompareEvent,
  CompareStarted,
  StoredMessage,
} from "~/lib/types";

import {
  chatReducer,
  createChatState,
  replyAnnouncement,
} from "./chat-reducer";
import type { ChatState } from "./chat-reducer";
import { DEFAULT_SETTINGS } from "./default-settings";
import { fromStored, splitStored } from "./stored-messages";
import { holdUntilReleased } from "./use-chat-stream";

vi.mock("~/lib/ipc");

const stored = (
  id: string,
  role: StoredMessage["role"],
  status: StoredMessage["status"] = "complete",
  extra: Partial<StoredMessage> = {}
): StoredMessage => ({
  id,
  role,
  content: id,
  status,
  createdAt: "2026-01-01T00:00:00.000Z",
  promptTokens: null,
  completionTokens: null,
  durationMs: null,
  thinking: null,
  thinkingMs: null,
  evalMs: null,
  model: null,
  compareGroup: null,
  ...extra,
});

const started: CompareStarted = {
  requestId: "r1",
  userMessageId: "u1",
  compareGroup: "g1",
  candidates: [
    { messageId: "m1", model: "small" },
    { messageId: "m2", model: "large" },
  ],
};

/** A saved chat with one answered turn. */
const savedChat = (): ChatState =>
  chatReducer(
    chatReducer(createChatState(), { type: "open", conversationId: "c1" }),
    {
      type: "loaded",
      conversationId: "c1",
      messages: fromStored([stored("q", "user"), stored("a", "assistant")]),
      compare: null,
      settings: DEFAULT_SETTINGS,
    }
  );

const comparing = (): ChatState =>
  chatReducer(savedChat(), {
    type: "compare",
    streamId: "s1",
    text: "Which?",
    models: ["small", "large"],
  });

const candidateEvent = (
  messageId: string,
  event: Extract<CompareEvent, { type: "candidate" }>["event"]
): CompareEvent => ({ type: "candidate", messageId, event });

const done = {
  type: "done",
  promptTokens: 3,
  completionTokens: 2,
  durationMs: 10,
  doneReason: "stop",
  thinkingMs: null,
  evalMs: 5,
} as const;

const apply = (state: ChatState, ...events: CompareEvent[]): ChatState =>
  events.reduce(
    (current, event) =>
      chatReducer(current, {
        type: "compareEvent",
        streamId: "s1",
        event,
        at: 0,
      }),
    state
  );

const adopted = (): ChatState =>
  chatReducer(comparing(), { type: "compareStarted", streamId: "s1", started });

describe("comparing models in the reducer", () => {
  it("starts with the prompt and one waiting column per model", () => {
    const state = comparing();
    expect(state.phase).toBe("comparing");
    expect(state.messages).toHaveLength(2);
    expect(state.compare?.prompt.content).toBe("Which?");
    expect(
      state.compare?.candidates.map(({ model, status }) => [model, status])
    ).toEqual([
      ["small", "streaming"],
      ["large", "streaming"],
    ]);
    expect(replyAnnouncement(state)).toBe("Models are answering…");
  });

  it("refuses a comparison in temporary or unsaved chats", () => {
    const action = {
      type: "compare",
      streamId: "s1",
      text: "x",
      models: ["a", "b"],
    } as const;
    const unsaved = createChatState();
    expect(chatReducer(unsaved, action)).toBe(unsaved);
    const temporary = chatReducer(unsaved, { type: "openTemporary" });
    expect(chatReducer(temporary, action)).toBe(temporary);
  });

  it("adopts Rust's ids and streams each event into its own column", () => {
    const state = apply(
      adopted(),
      candidateEvent("m1", { type: "token", content: "Small says" }),
      candidateEvent("m1", done),
      candidateEvent("m2", { type: "thinking", content: "Hmm" }),
      candidateEvent("m2", { type: "token", content: "Large" })
    );
    const [small, large] = state.compare?.candidates ?? [];
    expect(state.requestId).toBe("r1");
    expect(state.compare?.group).toBe("g1");
    expect(state.compare?.prompt.id).toBe("u1");
    expect(small).toMatchObject({
      id: "m1",
      content: "Small says",
      status: "complete",
      stats: { completionTokens: 2, durationMs: 10, evalMs: 5 },
    });
    expect(large).toMatchObject({
      id: "m2",
      content: "Large",
      thinking: "Hmm",
      status: "streaming",
    });
    expect(state.phase).toBe("comparing");
  });

  it("drops events that arrive before the ids, which is why the hook holds them", () => {
    const early = apply(
      comparing(),
      candidateEvent("m1", { type: "token", content: "lost" })
    );
    expect(early.compare?.candidates[0]?.content).toBe("");

    // What the hook does: hold, adopt, then release.
    const held: CompareEvent[] = [];
    const events = holdUntilReleased((event: CompareEvent) => {
      held.push(event);
    });
    events.push(candidateEvent("m1", { type: "token", content: "kept" }));
    expect(held).toEqual([]);
    let state = adopted();
    events.release();
    state = apply(state, ...held);
    expect(state.compare?.candidates[0]?.content).toBe("kept");
    // Later events go straight through.
    events.push({ type: "finished" });
    expect(held.at(-1)).toEqual({ type: "finished" });
  });

  it("an error fails only its own column; finished ends the run", () => {
    const error: AppError = { kind: "ollama", message: "no such model" };
    let state = apply(
      adopted(),
      candidateEvent("m1", { type: "error", error }),
      candidateEvent("m2", done)
    );
    expect(state.compare?.candidates.map(({ status }) => status)).toEqual([
      "error",
      "complete",
    ]);
    expect(state.compare?.candidates[0]?.failure).toBe("no such model");
    expect(state.error).toBeNull();
    state = apply(state, { type: "finished" });
    expect(state.phase).toBe("idle");
    expect(state.streamId).toBeNull();
    expect(state.settledStreamId).toBe("s1");
  });

  it("Stop marks the running column stopped and the waiting ones failed", () => {
    const running = apply(
      adopted(),
      candidateEvent("m1", { type: "token", content: "Part" })
    );
    const stopped = chatReducer(running, { type: "cancel" });
    expect(stopped.phase).toBe("idle");
    expect(stopped.compare?.candidates.map(({ status }) => status)).toEqual([
      "cancelled",
      "error",
    ]);
    // The late `finished` of the stopped run changes nothing.
    expect(apply(stopped, { type: "finished" })).toBe(stopped);
  });

  it("refuses sends, regenerates, edits and new comparisons until a pick", () => {
    const finished = apply(
      adopted(),
      candidateEvent("m1", done),
      candidateEvent("m2", done),
      { type: "finished" }
    );
    for (const action of [
      { type: "send", streamId: "s2", text: "Next" },
      { type: "regenerate", streamId: "s2" },
      { type: "edit", streamId: "s2", messageId: "q", text: "x" },
      { type: "compare", streamId: "s2", text: "x", models: ["a", "b"] },
    ] as const) {
      const refused = chatReducer(finished, action);
      expect(refused.streamId).toBeNull();
      expect(refused.notice).toBe("compare");
      // The comparison stays, so it can still be picked or discarded.
      expect(refused.compare).toBe(finished.compare);
    }
    // While it runs, the same actions are busy.
    expect(
      chatReducer(adopted(), { type: "send", streamId: "s2", text: "x" }).notice
    ).toBe("busy");
  });

  it("a refused comparison shows the prompt as failed", () => {
    const error: AppError = { kind: "invalidInput", message: "nope" };
    const state = chatReducer(comparing(), {
      type: "compareFailed",
      streamId: "s1",
      error,
    });
    expect(state.phase).toBe("idle");
    expect(state.compare).toBeNull();
    expect(state.messages.at(-1)).toMatchObject({
      content: "Which?",
      status: "error",
    });
    expect(state.error).toEqual(error);
  });

  it("a refresh after a pick shows the kept answer as a reply", () => {
    const finished = apply(adopted(), { type: "finished" });
    const kept = fromStored([
      stored("q", "user"),
      stored("a", "assistant"),
      stored("u1", "user"),
      stored("m2", "assistant", "complete", { model: "large" }),
    ]);
    const state = chatReducer(finished, {
      type: "refreshed",
      conversationId: "c1",
      messages: kept,
      compare: null,
    });
    expect(state.compare).toBeNull();
    expect(state.messages.at(-1)).toMatchObject({ id: "m2", model: "large" });
    // A refresh for another conversation is ignored.
    expect(
      chatReducer(finished, {
        type: "refreshed",
        conversationId: "c2",
        messages: [],
        compare: null,
      })
    ).toBe(finished);
  });

  it("labels a live reply with the model it was sent to", () => {
    const state = chatReducer(savedChat(), {
      type: "send",
      streamId: "s2",
      text: "Hi",
      model: "small",
    });
    expect(state.messages.at(-1)).toMatchObject({
      role: "assistant",
      model: "small",
    });
  });
});

describe("splitStored", () => {
  it("separates an unpicked comparison from the conversation", () => {
    const group = { compareGroup: "g1" };
    const { messages, compare } = splitStored([
      stored("q", "user"),
      stored("a", "assistant", "complete", { model: "small" }),
      stored("u1", "user", "complete", group),
      stored("m1", "assistant", "complete", { ...group, model: "small" }),
      stored("m2", "assistant", "pending", { ...group, model: "large" }),
    ]);
    expect(messages.map(({ id }) => id)).toEqual(["q", "a"]);
    expect(messages[1]?.model).toBe("small");
    expect(compare?.group).toBe("g1");
    expect(compare?.prompt.id).toBe("u1");
    expect(
      compare?.candidates.map(({ id, model, status }) => [id, model, status])
    ).toEqual([
      ["m1", "small", "complete"],
      ["m2", "large", "streaming"],
    ]);
  });

  it("has no comparison and no labels for older chats", () => {
    const { messages, compare } = splitStored([
      stored("q", "user"),
      stored("a", "assistant"),
    ]);
    expect(compare).toBeNull();
    expect(messages[1]?.model).toBeUndefined();
  });
});
