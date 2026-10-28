import { describe, expect, it, vi } from "vitest";

import type { ChartSpec, ChatEvent, StoredMessage } from "~/lib/types";

import type { ChatState } from "./chat-reducer";
import { chatReducer, createChatState } from "./chat-reducer";
import { fromStored } from "./stored-messages";

vi.mock("~/lib/ipc");

const sent = (): ChatState =>
  chatReducer(createChatState("c1"), {
    type: "send",
    streamId: "s1",
    text: "Chart it",
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
  evalMs: null,
};

const chart: ChartSpec = {
  title: "Pop",
  type: "bar",
  labels: ["1950"],
  values: [4],
};

const row = (id: string, charts: StoredMessage["charts"]): StoredMessage => ({
  id,
  role: "assistant",
  content: "Here: Grew.",
  status: "complete",
  createdAt: "2026-01-01T00:00:00.000Z",
  promptTokens: null,
  completionTokens: null,
  durationMs: null,
  thinking: null,
  thinkingMs: null,
  evalMs: null,
  model: null,
  compareGroup: null,
  charts,
  fileLookups: [],
});

describe("charts", () => {
  it("attaches charts and refused calls to the streaming reply", () => {
    let state = event(sent(), { type: "token", content: "Here:" });
    state = event(state, { type: "chart", offset: 5, spec: chart });
    state = event(state, { type: "chartError", offset: 5, reason: "why" });
    state = event(state, { type: "token", content: " Grew." });
    const reply = state.messages.at(-1);
    expect(reply?.content).toBe("Here: Grew.");
    expect(reply?.status).toBe("streaming");
    expect(reply?.charts).toEqual([
      { offset: 5, spec: chart },
      { offset: 5, spec: null, error: "why" },
    ]);
    // The charts stay once the reply completes.
    expect(event(state, done).messages.at(-1)?.charts).toHaveLength(2);
  });

  it("loads saved charts with their reply, and none for others", () => {
    const [charted, plain] = fromStored([
      row("a", [{ offset: 5, spec: chart }]),
      row("b", []),
    ]);
    expect(charted?.charts).toEqual([{ offset: 5, spec: chart }]);
    expect(plain).not.toHaveProperty("charts");
  });
});
