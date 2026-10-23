import { beforeEach, describe, expect, it, vi } from "vitest";

import { conversationUpdateSettings, tempChatUpdateSettings } from "~/lib/ipc";
import type { AppError, Conversation, ConversationSettings } from "~/lib/types";

import { DEFAULT_SETTINGS } from "./default-settings";
import { settingsSync } from "./settings-sync";

vi.mock("~/lib/ipc");

const finnish: ConversationSettings = {
  systemPrompt: "Answer only in Finnish",
  options: {},
  think: { kind: "off" },
  charts: false,
};

const hot: ConversationSettings = {
  systemPrompt: "",
  options: { temperature: 1.9 },
  think: { kind: "on" },
  charts: true,
};

const refused: AppError = { kind: "invalidInput", message: "too hot" };

const saved = (id: string): Conversation => ({
  id,
  title: "t",
  model: "m",
  createdAt: "x",
  updatedAt: "x",
  settings: DEFAULT_SETTINGS,
});

describe("settingsSync", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(conversationUpdateSettings).mockImplementation((id) =>
      Promise.resolve(saved(id))
    );
    vi.mocked(tempChatUpdateSettings).mockResolvedValue();
  });

  it("saves a saved chat's settings", async () => {
    const sync = settingsSync();
    sync.open("c1");
    const onFailed = vi.fn();

    sync.save(finnish, onFailed);
    await sync.settled();

    expect(conversationUpdateSettings).toHaveBeenCalledWith("c1", finnish);
    expect(sync.current().saved).toEqual(finnish);
    expect(onFailed).not.toHaveBeenCalled();
  });

  it("keeps an unsaved chat's settings for its first send", async () => {
    const sync = settingsSync();
    sync.save(finnish, vi.fn());
    await sync.settled();

    expect(conversationUpdateSettings).not.toHaveBeenCalled();
    expect(tempChatUpdateSettings).not.toHaveBeenCalled();
  });

  it("saves a change made while the chat is being created to the new id", async () => {
    const sync = settingsSync();
    const chat = sync.open(null);
    // The first send is creating the conversation.
    const created = Promise.withResolvers<string>();
    chat.started = created.promise;

    sync.save(finnish, vi.fn());
    await Promise.resolve();
    expect(conversationUpdateSettings).not.toHaveBeenCalled();
    // What the send's `onCreated` does, before its start settles.
    chat.conversationId = "c1";
    created.resolve("r1");
    await sync.settled();

    expect(conversationUpdateSettings).toHaveBeenCalledWith("c1", finnish);
  });

  it("saves a change made while a temporary session starts to that session", async () => {
    const sync = settingsSync();
    const chat = sync.open(null);
    const session = Promise.withResolvers<string>();
    chat.session = session.promise;

    sync.save(hot, vi.fn());
    session.resolve("t1");
    await sync.settled();

    expect(tempChatUpdateSettings).toHaveBeenCalledWith("t1", hot);
  });

  it("does not save to a temporary session that never started", async () => {
    const sync = settingsSync();
    const chat = sync.open(null);
    chat.session = Promise.reject(new Error("busy"));
    const onFailed = vi.fn();

    sync.save(hot, onFailed);
    await sync.settled();

    expect(tempChatUpdateSettings).not.toHaveBeenCalled();
    expect(onFailed).not.toHaveBeenCalled();
  });

  it("shows a failed save's error and restores, then keeps saving", async () => {
    vi.mocked(conversationUpdateSettings).mockRejectedValueOnce(refused);
    const sync = settingsSync();
    sync.open("c1").saved = finnish;
    const onFailed = vi.fn();

    sync.save(hot, onFailed);
    await sync.settled();
    expect(onFailed).toHaveBeenCalledWith(finnish, refused);

    // The chain recovered: the next save goes through, and so would a send.
    sync.save(DEFAULT_SETTINGS, onFailed);
    await sync.settled();
    expect(conversationUpdateSettings).toHaveBeenLastCalledWith(
      "c1",
      DEFAULT_SETTINGS
    );
    expect(onFailed).toHaveBeenCalledTimes(1);
  });

  it("leaves another chat alone when an earlier chat's save fails", async () => {
    vi.mocked(conversationUpdateSettings).mockRejectedValueOnce(refused);
    const sync = settingsSync();
    sync.open("a");
    const onFailed = vi.fn();

    sync.save(hot, onFailed);
    // The user moves to chat B before chat A's save fails.
    sync.open("b");
    await sync.settled();

    expect(conversationUpdateSettings).toHaveBeenCalledWith("a", hot);
    expect(onFailed).not.toHaveBeenCalled();
  });

  it("does not let an older failed save undo a newer one", async () => {
    vi.mocked(conversationUpdateSettings).mockRejectedValueOnce(refused);
    const sync = settingsSync();
    sync.open("c1");
    const onFailed = vi.fn();

    sync.save(hot, onFailed);
    sync.save(finnish, onFailed);
    await sync.settled();

    expect(onFailed).not.toHaveBeenCalled();
    expect(sync.current().saved).toEqual(finnish);
  });

  it("does not wait for a send started after the change", async () => {
    const first = Promise.withResolvers<Conversation>();
    vi.mocked(conversationUpdateSettings).mockReturnValueOnce(first.promise);
    const sync = settingsSync();
    const chat = sync.open("c1");
    sync.save(hot, vi.fn());
    sync.save(finnish, vi.fn());
    // A send that waits for both saves replaces `started`.
    const send = Promise.withResolvers<string>();
    chat.started = sync.settled().then(() => send.promise);

    first.resolve(saved("c1"));
    await sync.settled();
    send.resolve("r1");

    expect(conversationUpdateSettings).toHaveBeenLastCalledWith("c1", finnish);
  });

  it("settles only after a save in flight", async () => {
    const pending = Promise.withResolvers<Conversation>();
    vi.mocked(conversationUpdateSettings).mockReturnValueOnce(pending.promise);
    const sync = settingsSync();
    sync.open("c1");
    sync.save(finnish, vi.fn());
    let settled = false;
    void sync.settled().then(() => {
      settled = true;
    });

    await Promise.resolve();
    await Promise.resolve();
    expect(settled).toBe(false);
    pending.resolve(saved("c1"));
    await sync.settled();
    expect(settled).toBe(true);
  });
});
