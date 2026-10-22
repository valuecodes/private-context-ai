import { DEFAULT_SETTINGS } from "~/features/chat/default-settings";
import { toAppError } from "~/lib/app-error";
import { conversationUpdateSettings, tempChatUpdateSettings } from "~/lib/ipc";
import type { AppError, ConversationSettings } from "~/lib/types";

/**
 * The chat a settings save belongs to. Replaced whenever another chat opens;
 * a send's callbacks update the one it was made in, so a save still reaches
 * that chat after the view has moved on.
 */
type OpenChat = {
  /** Set once the first send has created the conversation. */
  conversationId: string | null;
  /** A temporary chat's session, once a send has started it. */
  session: Promise<string> | null;
  /** The latest send's start; settles either way, never rejects. */
  started: Promise<unknown>;
  /** What Rust holds for this chat, as far as this view knows. */
  saved: ConversationSettings;
  /** Bumped by every change: only the latest save may roll back. */
  revision: number;
};

const openChat = (conversationId: string | null): OpenChat => ({
  conversationId,
  session: null,
  started: Promise.resolve(),
  saved: DEFAULT_SETTINGS,
  revision: 0,
});

/**
 * Saves `settings` where `chat` keeps them: its conversation, or its
 * temporary session. An unsaved chat (or one whose session never started)
 * passes them with its next first send instead.
 */
const store = (
  chat: OpenChat,
  settings: ConversationSettings
): Promise<void> => {
  if (chat.conversationId !== null) {
    return conversationUpdateSettings(chat.conversationId, settings).then(
      () => undefined
    );
  }
  if (chat.session !== null) {
    return chat.session.then(
      (sessionId) => tempChatUpdateSettings(sessionId, settings),
      () => undefined
    );
  }
  return Promise.resolve();
};

/** A failed save: the settings to show again, and why. */
type OnFailed = (saved: ConversationSettings, error: AppError) => void;

/**
 * Keeps the open chat's settings in step with Rust. Saves run one at a time
 * and in order, each after its chat's latest send has started, so a chat
 * still being created gets them too. The chain never rejects, so a failed
 * save holds up neither later saves nor sends.
 */
const settingsSync = () => {
  let current = openChat(null);
  let chain: Promise<void> = Promise.resolve();
  return {
    /** Starts tracking a newly opened chat. */
    open: (conversationId: string | null): OpenChat => {
      current = openChat(conversationId);
      return current;
    },
    current: (): OpenChat => current,
    /**
     * Saves `settings` for the open chat. `onFailed` runs only if that chat
     * is still open and nothing changed its settings since.
     */
    save: (settings: ConversationSettings, onFailed: OnFailed) => {
      const target = current;
      target.revision += 1;
      // Read now: a later send replaces `started` and waits for this save.
      const { revision, started } = target;
      chain = chain
        .then(() => started)
        .then(() => store(target, settings))
        .then(
          () => {
            target.saved = settings;
          },
          (error: unknown) => {
            if (current === target && target.revision === revision) {
              onFailed(target.saved, toAppError(error));
            }
          }
        );
    },
    /** Settles once every save so far has finished. */
    settled: (): Promise<void> => chain,
  };
};

type SettingsSync = ReturnType<typeof settingsSync>;

export type { SettingsSync };
export { settingsSync };
