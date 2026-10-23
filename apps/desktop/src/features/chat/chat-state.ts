import { DEFAULT_SETTINGS } from "~/features/chat/default-settings";
import type { UiMessage } from "~/features/chat/ui-message";
import type {
  AppError,
  ChatEvent,
  CompareEvent,
  CompareStarted,
  ConversationSettings,
  RequestId,
  SendStarted,
} from "~/lib/types";

/** A comparison waiting for a pick: the prompt and one reply per model. */
type CompareState = {
  /** `null` until Rust's ids arrive. */
  group: string | null;
  prompt: UiMessage;
  /** In run order, each with its `model`. */
  candidates: UiMessage[];
};

/**
 * `busy`: a send while a reply streams. `length`: the reply was cut off.
 * `compare`: a send while a comparison waits for a pick.
 */
type ChatNotice = "busy" | "length" | "compare";

type ChatState = {
  /** The open conversation; `null` for a new chat the first send saves. */
  conversationId: string | null;
  /** A temporary chat: kept in Rust's memory only, never saved. */
  temporary: boolean;
  /** The chat's project, whose instructions it is sent; `null` outside one. */
  projectId: string | null;
  /** The saved messages are still loading; sending waits for them. */
  loading: boolean;
  messages: UiMessage[];
  /** `comparing`: models answer a comparison one after another. */
  phase: "idle" | "streaming" | "comparing";
  /** Shown after the messages; sending waits until it is resolved. */
  compare: CompareState | null;
  /** Local id of the current send; events for any other id are stale. */
  streamId: string | null;
  /** The send that ended last: only its reload may replace the messages. */
  settledStreamId: string | null;
  /** The streaming reply's bubble: a local id until Rust's arrives. */
  replyId: string | null;
  requestId: RequestId | null;
  /**
   * The messages before a regenerate or edit truncated them, shown again if
   * its reply fails or is stopped.
   */
  revision: UiMessage[] | null;
  error: AppError | null;
  notice: ChatNotice | null;
  /** Applied from the next send; a saved chat's load with its messages. */
  settings: ConversationSettings;
};

/** `projectId`: the chat's project; outside any project when left out. */
type ChatAction =
  | { type: "open"; conversationId: string | null; projectId?: string | null }
  | { type: "openTemporary"; projectId?: string | null }
  | {
      type: "loaded";
      conversationId: string;
      messages: UiMessage[];
      compare: CompareState | null;
      settings: ConversationSettings;
    }
  | { type: "loadFailed"; conversationId: string; error: AppError }
  /**
   * The saved messages again, after the regenerate, edit or comparison
   * `streamId` was refused, stopped or ended.
   */
  | {
      type: "reloaded";
      conversationId: string;
      streamId: string;
      messages: UiMessage[];
      compare: CompareState | null;
    }
  /** The saved messages again, after a pick or discard. */
  | {
      type: "refreshed";
      conversationId: string;
      messages: UiMessage[];
      compare: CompareState | null;
    }
  /** `model` labels the new reply. */
  | { type: "send"; streamId: string; text: string; model?: string }
  | { type: "regenerate"; streamId: string; model?: string }
  | {
      type: "edit";
      streamId: string;
      messageId: string;
      text: string;
      model?: string;
    }
  | {
      type: "compare";
      streamId: string;
      text: string;
      models: readonly string[];
    }
  | { type: "compareStarted"; streamId: string; started: CompareStarted }
  | { type: "compareEvent"; streamId: string; event: CompareEvent; at: number }
  | { type: "compareFailed"; streamId: string; error: AppError }
  /** A pick or discard was refused. */
  | { type: "refused"; conversationId: string; error: AppError }
  | { type: "created"; streamId: string; conversationId: string }
  | { type: "started"; streamId: string; started: SendStarted }
  /** `at`: when the event arrived, in ms, so the reducer stays pure. */
  | { type: "event"; streamId: string; event: ChatEvent; at: number }
  | { type: "cancel" }
  | { type: "failed"; streamId: string; error: AppError }
  | { type: "deleted"; conversationId: string }
  /** Rust deleted the project, its chats and its temporary chats. */
  | { type: "projectDeleted"; projectId: string }
  /** The open chat's settings changed; `error` when saving them failed. */
  | { type: "settings"; settings: ConversationSettings; error?: AppError };

type TurnAction = Extract<
  ChatAction,
  { type: "send" } | { type: "regenerate" } | { type: "edit" }
>;

const createChatState = (
  conversationId: string | null = null,
  temporary = false,
  projectId: string | null = null
): ChatState => ({
  conversationId,
  temporary,
  projectId,
  loading: false,
  messages: [],
  phase: "idle",
  compare: null,
  streamId: null,
  settledStreamId: null,
  replyId: null,
  requestId: null,
  revision: null,
  error: null,
  notice: null,
  settings: DEFAULT_SETTINGS,
});

const userId = (streamId: string): string => `${streamId}:user`;

const userBubble = (streamId: string, text: string): UiMessage => ({
  id: userId(streamId),
  role: "user",
  content: text,
  status: "complete",
  local: true,
});

export type { ChatAction, ChatState, CompareState, TurnAction };
export { createChatState, userBubble, userId };
