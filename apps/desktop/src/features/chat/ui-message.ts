import type { ReplyStats } from "~/features/chat/message-stats";
import type { ChartEntry, ChatEvent, FileLookup } from "~/lib/types";

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
  /** Why a compared reply failed, while it is shown live. */
  failure?: string;
  /** A reply's `render_chart` calls, when it made any. */
  charts?: ChartEntry[];
  /** A reply's file tool calls, when it made any. */
  fileLookups?: FileLookup[];
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

/** A reply with a chart, or a refused chart call, added. */
const withChart = (
  message: UiMessage,
  event: Extract<ChatEvent, { type: "chart" | "chartError" }>
): UiMessage => {
  const entry: ChartEntry =
    event.type === "chart"
      ? { offset: event.offset, spec: event.spec }
      : { offset: event.offset, spec: null, error: event.reason };
  return { ...message, charts: [...(message.charts ?? []), entry] };
};

/** A reply with a file lookup added. */
const withFileLookup = (message: UiMessage, lookup: FileLookup): UiMessage => ({
  ...message,
  fileLookups: [...(message.fileLookups ?? []), lookup],
});

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
export { updateMessage, withChart, withChunk, withFileLookup, withStats };
