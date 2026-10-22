import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatEdit,
  chatRegenerate,
  chatSend,
  conversationCreate,
  tempChatDiscard,
  tempChatEdit,
  tempChatRegenerate,
  tempChatSend,
  tempChatStart,
} from "~/lib/ipc";

import { startRequest, tempSession } from "./send-request";

vi.mock("~/lib/ipc");

const request = {
  model: "m",
  think: null,
  input: { kind: "send", prompt: "Hi" },
} as const;

const started = {
  requestId: "r1",
  userMessageId: "u1",
  assistantMessageId: "a1",
};

const handlers = () => ({ onCreated: vi.fn(), onStarted: vi.fn() });

const ignore = (): void => undefined;

const flush = () =>
  new Promise((resolve) => {
    setTimeout(resolve, 0);
  });

describe("regenerate and edit requests", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    for (const send of [
      chatRegenerate,
      chatEdit,
      tempChatRegenerate,
      tempChatEdit,
    ]) {
      vi.mocked(send).mockResolvedValue(started);
    }
  });

  it("routes a saved chat's regenerate and edit", async () => {
    const onEvent = vi.fn();
    const saved = { kind: "saved", conversationId: "c1" } as const;
    const callbacks = handlers();

    expect(
      await startRequest(
        saved,
        { model: "m", think: true, input: { kind: "regenerate" } },
        onEvent,
        callbacks
      )
    ).toEqual(started);
    await startRequest(
      saved,
      {
        model: "m",
        think: null,
        input: { kind: "edit", messageId: "u0", prompt: "Changed" },
      },
      onEvent,
      callbacks
    );

    expect(chatRegenerate).toHaveBeenCalledWith(
      { conversationId: "c1", model: "m", think: true },
      onEvent
    );
    expect(chatEdit).toHaveBeenCalledWith(
      {
        conversationId: "c1",
        messageId: "u0",
        model: "m",
        prompt: "Changed",
        think: null,
      },
      onEvent
    );
    expect(chatSend).not.toHaveBeenCalled();
    expect(callbacks.onStarted).toHaveBeenCalledWith("c1", "m");
  });

  it("routes a temporary chat's regenerate and edit", async () => {
    const onEvent = vi.fn();
    const temporary = {
      kind: "temporary",
      sessionId: Promise.resolve("t1"),
    } as const;

    await startRequest(
      temporary,
      { model: "m", think: null, input: { kind: "regenerate" } },
      onEvent,
      handlers()
    );
    await startRequest(
      temporary,
      {
        model: "m",
        think: null,
        input: { kind: "edit", messageId: "u0", prompt: "Changed" },
      },
      onEvent,
      handlers()
    );

    expect(tempChatRegenerate).toHaveBeenCalledWith(
      { sessionId: "t1", model: "m", think: null },
      onEvent
    );
    expect(tempChatEdit).toHaveBeenCalledWith(
      {
        sessionId: "t1",
        messageId: "u0",
        model: "m",
        prompt: "Changed",
        think: null,
      },
      onEvent
    );
    expect(chatRegenerate).not.toHaveBeenCalled();
    expect(chatEdit).not.toHaveBeenCalled();
  });
});

describe("temporary chat requests", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(tempChatSend).mockResolvedValue(started);
    vi.mocked(tempChatDiscard).mockResolvedValue();
  });

  it("sends through the session and never saves", async () => {
    const onEvent = vi.fn();
    const callbacks = handlers();

    const result = await startRequest(
      { kind: "temporary", sessionId: Promise.resolve("t1") },
      request,
      onEvent,
      callbacks
    );

    expect(result).toEqual(started);
    expect(tempChatSend).toHaveBeenCalledWith(
      { sessionId: "t1", model: "m", prompt: "Hi", think: null },
      onEvent
    );
    expect(conversationCreate).not.toHaveBeenCalled();
    expect(chatSend).not.toHaveBeenCalled();
    expect(callbacks.onCreated).not.toHaveBeenCalled();
    expect(callbacks.onStarted).not.toHaveBeenCalled();
  });

  it("starts one session and reuses it", async () => {
    vi.mocked(tempChatStart).mockResolvedValue("t1");
    const session = tempSession();

    expect(await session.id()).toBe("t1");
    expect(await session.id()).toBe("t1");
    expect(tempChatStart).toHaveBeenCalledTimes(1);
  });

  it("discards a session still being created, then starts a fresh one", async () => {
    let resolveFirst: (id: string) => void = ignore;
    vi.mocked(tempChatStart)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveFirst = resolve;
        })
      )
      .mockResolvedValueOnce("t2");
    const session = tempSession();
    void session.id();

    session.discard();
    const next = session.id();
    resolveFirst("t1");

    expect(await next).toBe("t2");
    await flush();
    expect(tempChatDiscard).toHaveBeenCalledWith("t1");
    expect(tempChatDiscard).not.toHaveBeenCalledWith("t2");
  });

  it("retries after a refused start", async () => {
    vi.mocked(tempChatStart)
      .mockRejectedValueOnce(new Error("busy"))
      .mockResolvedValueOnce("t2");
    const session = tempSession();

    await expect(session.id()).rejects.toThrow("busy");
    expect(await session.id()).toBe("t2");
  });

  it("does nothing on discard before any send", () => {
    tempSession().discard();
    expect(tempChatDiscard).not.toHaveBeenCalled();
  });
});
