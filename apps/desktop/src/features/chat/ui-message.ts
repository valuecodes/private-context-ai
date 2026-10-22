import type { ReplyStats } from "~/features/chat/message-stats";
import type { ChatEvent } from "~/lib/types";

type MessageStatus = "complete" | "streaming" | "cancelled" | "error";

type UiMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  status: MessageStatus;
  /** A reply streamed in this view, rather than loaded. */
  live?: true;
  /** A prompt sent in this view whose saved id has not arrived yet. */
  local?: true;
  /** The model's reasoning, when it sent any. */
  thinking?: string;
  /** How long it thought before answering; null when not known. */
  thinkingMs?: number | null;
  /** When the first thinking chunk arrived, while it streams. */
  thinkingStartedAt?: number;
  /** A reply's counts, once it has finished. */
  stats?: ReplyStats;
  /** The model that wrote a reply, when known. */
  model?: string;
};

const updateMessage = (
  messages: UiMessage[],
  id: string | null,
  update: (message: UiMessage) => UiMessage
): UiMessage[] =>
  messages.map((message) => (message.id === id ? update(message) : message));

/** The thinking time once the answer starts, if it was not known yet. */
const thoughtFor = (message: UiMessage, at: number): number | null => {
  if (message.thinkingMs !== undefined && message.thinkingMs !== null) {
    return message.thinkingMs;
  }
  return message.thinkingStartedAt === undefined
    ? null
    : at - message.thinkingStartedAt;
};

/** A reply with a thinking or answer chunk added, arrived at `at`. */
const withChunk = (
  message: UiMessage,
  event: Extract<ChatEvent, { type: "thinking" | "token" }>,
  at: number
): UiMessage =>
  event.type === "thinking"
    ? {
        ...message,
        thinking: (message.thinking ?? "") + event.content,
        thinkingStartedAt: message.thinkingStartedAt ?? at,
      }
    : {
        ...message,
        content: message.content + event.content,
        ...(message.thinking === undefined
          ? {}
          : { thinkingMs: thoughtFor(message, at) }),
      };

/** A finished reply with its counts. */
const withStats = (
  message: UiMessage,
  {
    completionTokens,
    durationMs,
    evalMs,
    thinkingMs,
  }: Extract<ChatEvent, { type: "done" }>
): UiMessage => ({
  ...message,
  stats: { completionTokens, durationMs, evalMs },
  ...(thinkingMs === null ? {} : { thinkingMs }),
});

export type { MessageStatus, UiMessage };
export { updateMessage, withChunk, withStats };
