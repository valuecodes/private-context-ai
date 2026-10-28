import { describe, expect, it, vi } from "vitest";

import type { ChatEvent, FileLookup, StoredMessage } from "~/lib/types";

import type { ChatState } from "./chat-reducer";
import { chatReducer, createChatState } from "./chat-reducer";
import { fromStored } from "./stored-messages";

vi.mock("~/lib/ipc");

const sent = (): ChatState =>
  chatReducer(createChatState("c1"), {
    type: "send",
    streamId: "s1",
    text: "What does the file say?",
  });

const event = (state: ChatState, chatEvent: ChatEvent): ChatState =>
  chatReducer(state, {
    type: "event",
    streamId: "s1",
    event: chatEvent,
    at: 0,
  });

const search: FileLookup = { tool: "search", target: "tax", hits: 2 };
const read: FileLookup = { tool: "read", target: "a.txt", hits: 40 };

const row = (
  id: string,
  fileLookups: StoredMessage["fileLookups"]
): StoredMessage => ({
  id,
  role: "assistant",
  content: "It says plover.",
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
  charts: [],
  fileLookups,
});

describe("file lookups", () => {
  it("attaches lookups to the streaming reply in order", () => {
    let state = event(sent(), { type: "fileLookup", lookup: search });
    state = event(state, { type: "fileLookup", lookup: read });
    state = event(state, { type: "token", content: "It says plover." });
    const reply = state.messages.at(-1);
    expect(reply?.status).toBe("streaming");
    expect(reply?.fileLookups).toEqual([search, read]);
  });

  it("loads saved lookups with their reply, and none for others", () => {
    const [looked, plain] = fromStored([row("a", [search]), row("b", [])]);
    expect(looked?.fileLookups).toEqual([search]);
    expect(plain).not.toHaveProperty("fileLookups");
  });
});
