import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatSend,
  conversationCreate,
  tempChatDiscard,
  tempChatSend,
  tempChatStart,
} from "~/lib/ipc";

import { startRequest, tempSession } from "./send-request";

vi.mock("~/lib/ipc");

const request = { model: "m", prompt: "Hi", think: null };

const ignore = (): void => undefined;

const flush = () =>
  new Promise((resolve) => {
    setTimeout(resolve, 0);
  });

describe("temporary chat requests", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(tempChatSend).mockResolvedValue("r1");
    vi.mocked(tempChatDiscard).mockResolvedValue();
  });

  it("sends through the session and never saves", async () => {
    const onEvent = vi.fn();
    const handlers = { onCreated: vi.fn(), onStarted: vi.fn() };

    const requestId = await startRequest(
      { kind: "temporary", sessionId: Promise.resolve("t1") },
      request,
      onEvent,
      handlers
    );

    expect(requestId).toBe("r1");
    expect(tempChatSend).toHaveBeenCalledWith(
      { sessionId: "t1", ...request },
      onEvent
    );
    expect(conversationCreate).not.toHaveBeenCalled();
    expect(chatSend).not.toHaveBeenCalled();
    expect(handlers.onCreated).not.toHaveBeenCalled();
    expect(handlers.onStarted).not.toHaveBeenCalled();
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
