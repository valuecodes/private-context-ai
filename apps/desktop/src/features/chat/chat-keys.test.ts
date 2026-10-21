import { describe, expect, it } from "vitest";

import { sendsMessage, stopsReply } from "./chat-keys";

const key = (
  name: string,
  changes: { shiftKey?: boolean; isComposing?: boolean } = {}
) => ({
  key: name,
  shiftKey: false,
  isComposing: false,
  ...changes,
});

const streaming = { streaming: true, inTextField: false };

describe("sendsMessage", () => {
  it("sends on Enter", () => {
    expect(sendsMessage(key("Enter"))).toBe(true);
  });

  it("leaves Shift+Enter to insert a newline", () => {
    expect(sendsMessage(key("Enter", { shiftKey: true }))).toBe(false);
  });

  it("ignores Enter while an IME composition is active", () => {
    expect(sendsMessage(key("Enter", { isComposing: true }))).toBe(false);
  });

  it("ignores other keys", () => {
    expect(sendsMessage(key("a"))).toBe(false);
    expect(sendsMessage(key("Escape"))).toBe(false);
  });
});

describe("stopsReply", () => {
  it("stops on Escape while a reply streams", () => {
    expect(stopsReply(key("Escape"), streaming)).toBe(true);
  });

  it("ignores Escape when idle, while composing, or in a text input", () => {
    expect(
      stopsReply(key("Escape"), { streaming: false, inTextField: false })
    ).toBe(false);
    expect(stopsReply(key("Escape", { isComposing: true }), streaming)).toBe(
      false
    );
    expect(
      stopsReply(key("Escape"), { streaming: true, inTextField: true })
    ).toBe(false);
  });

  it("ignores other keys", () => {
    expect(stopsReply(key("Enter"), streaming)).toBe(false);
  });
});
