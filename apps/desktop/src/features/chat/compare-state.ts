import { userBubble } from "~/features/chat/chat-state";
import type { ChatAction, ChatState } from "~/features/chat/chat-state";
import {
  updateMessage,
  withChunk,
  withStats,
} from "~/features/chat/ui-message";
import type { UiMessage } from "~/features/chat/ui-message";
import type { AppError, CompareEvent, CompareStarted } from "~/lib/types";

const candidateId = (streamId: string, index: number): string =>
  `${streamId}:candidate:${String(index)}`;

/**
 * Starts a comparison of a saved chat, or refuses it like a turn. A
 * temporary or unsaved chat cannot compare.
 */
const startCompare = (
  state: ChatState,
  { streamId, text, models }: Extract<ChatAction, { type: "compare" }>
): ChatState => {
  if (state.temporary || state.conversationId === null) {
    return state;
  }
  if (state.phase !== "idle") {
    return { ...state, notice: "busy" };
  }
  if (state.compare !== null) {
    return { ...state, notice: "compare" };
  }
  if (state.loading) {
    return state;
  }
  return {
    ...state,
    phase: "comparing",
    streamId,
    replyId: null,
    requestId: null,
    revision: null,
    error: null,
    notice: null,
    compare: {
      group: null,
      prompt: userBubble(streamId, text),
      candidates: models.map((model, index) => ({
        id: candidateId(streamId, index),
        role: "assistant",
        content: "",
        status: "streaming",
        live: true,
        model,
      })),
    },
  };
};

/** Takes Rust's ids, matched to the candidates by run order. */
const adoptCompareIds = (
  state: ChatState,
  { requestId, userMessageId, compareGroup, candidates }: CompareStarted
): ChatState => {
  const { compare } = state;
  if (compare === null) {
    return state;
  }
  return {
    ...state,
    requestId,
    compare: {
      group: compareGroup,
      prompt: { ...compare.prompt, id: userMessageId, local: undefined },
      candidates: compare.candidates.map((candidate, index) => {
        const saved = candidates.at(index);
        return saved === undefined
          ? candidate
          : { ...candidate, id: saved.messageId };
      }),
    },
  };
};

/**
 * Ends a comparison. Candidates still waiting were skipped; the one
 * running when it was stopped ends as stopped.
 */
const finishCompare = (state: ChatState, stopped: boolean): ChatState => {
  const { compare } = state;
  const running = compare?.candidates.findIndex(
    ({ status }) => status === "streaming"
  );
  return {
    ...state,
    phase: "idle",
    streamId: null,
    settledStreamId: state.streamId,
    requestId: null,
    compare:
      compare === null
        ? null
        : {
            ...compare,
            candidates: compare.candidates.map((candidate, index) => {
              if (candidate.status !== "streaming") {
                return candidate;
              }
              return {
                ...candidate,
                status: stopped && index === running ? "cancelled" : "error",
              };
            }),
          },
  };
};

/** One candidate's event goes to its own column; `finished` ends the run. */
const applyCompareEvent = (
  state: ChatState,
  event: CompareEvent,
  at: number
): ChatState => {
  const { compare } = state;
  if (event.type === "finished" || compare === null) {
    return finishCompare(state, false);
  }
  const { messageId, event: chat } = event;
  const update = (candidate: UiMessage): UiMessage => {
    if (chat.type === "thinking" || chat.type === "token") {
      return withChunk(candidate, chat, at);
    }
    if (chat.type === "done") {
      return { ...withStats(candidate, chat), status: "complete" };
    }
    return { ...candidate, status: "error", failure: chat.error.message };
  };
  return {
    ...state,
    compare: {
      ...compare,
      candidates: updateMessage(compare.candidates, messageId, update),
    },
  };
};

/**
 * Rust refused the comparison, so nothing was saved: the prompt shows as
 * failed, like a refused send.
 */
const failCompare = (state: ChatState, error: AppError): ChatState => {
  const busy = error.kind === "busy";
  return {
    ...state,
    messages:
      state.compare === null
        ? state.messages
        : [...state.messages, { ...state.compare.prompt, status: "error" }],
    phase: "idle",
    compare: null,
    streamId: null,
    settledStreamId: state.streamId,
    requestId: null,
    error: busy ? null : error,
    notice: busy ? "busy" : null,
  };
};

export {
  adoptCompareIds,
  applyCompareEvent,
  failCompare,
  finishCompare,
  startCompare,
};
