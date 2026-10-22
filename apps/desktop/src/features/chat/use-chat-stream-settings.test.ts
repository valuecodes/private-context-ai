import { describe, expect, it, vi } from "vitest";

import type { ConversationSettings } from "~/lib/types";

import { chatReducer, createChatState } from "./chat-reducer";
import { DEFAULT_SETTINGS } from "./default-settings";

vi.mock("~/lib/ipc");

const finnish: ConversationSettings = {
  systemPrompt: "Answer only in Finnish",
  options: { temperature: 0.7, num_ctx: 8192 },
  think: { kind: "level", level: "high" },
};

describe("chat settings in the reducer", () => {
  it("starts every new, opened and temporary chat with thinking off", () => {
    const changed = chatReducer(createChatState(), {
      type: "settings",
      settings: finnish,
    });
    expect(changed.settings).toEqual(finnish);
    expect(DEFAULT_SETTINGS.think).toEqual({ kind: "off" });

    for (const next of [
      chatReducer(changed, { type: "open", conversationId: null }),
      chatReducer(changed, { type: "open", conversationId: "c2" }),
      chatReducer(changed, { type: "openTemporary" }),
    ]) {
      expect(next.settings).toEqual(DEFAULT_SETTINGS);
    }
  });

  it("keeps a new chat's settings through its first send", () => {
    let state = chatReducer(createChatState(), {
      type: "settings",
      settings: finnish,
    });
    state = chatReducer(state, { type: "send", streamId: "s1", text: "Hi" });
    state = chatReducer(state, {
      type: "created",
      streamId: "s1",
      conversationId: "new",
    });
    expect(state.settings).toEqual(finnish);
  });

  it("adopts a loaded conversation's saved settings", () => {
    const loading = chatReducer(createChatState(), {
      type: "open",
      conversationId: "c1",
    });
    const state = chatReducer(loading, {
      type: "loaded",
      conversationId: "c1",
      messages: [],
      compare: null,
      settings: finnish,
    });
    expect(state.settings).toEqual(finnish);
  });

  it("shows a failed save's error with the settings Rust kept", () => {
    const error = { kind: "invalidInput", message: "too hot" } as const;
    const state = chatReducer(
      chatReducer(createChatState("c1"), {
        type: "settings",
        settings: finnish,
      }),
      { type: "settings", settings: DEFAULT_SETTINGS, error }
    );
    expect(state.settings).toEqual(DEFAULT_SETTINGS);
    expect(state.error).toEqual(error);

    // The next change clears it.
    const retried = chatReducer(state, { type: "settings", settings: finnish });
    expect(retried.error).toBeNull();
  });
});
