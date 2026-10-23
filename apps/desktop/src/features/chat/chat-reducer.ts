import {
  createChatState,
  userBubble,
  userId,
} from "~/features/chat/chat-state";
import type {
  ChatAction,
  ChatState,
  TurnAction,
} from "~/features/chat/chat-state";
import {
  adoptCompareIds,
  applyCompareEvent,
  failCompare,
  finishCompare,
  startCompare,
} from "~/features/chat/compare-state";
import {
  updateMessage,
  withChart,
  withChunk,
  withStats,
} from "~/features/chat/ui-message";
import type { MessageStatus, UiMessage } from "~/features/chat/ui-message";
import type { AppError, ChatEvent, SendStarted } from "~/lib/types";

const assistantId = (streamId: string): string => `${streamId}:assistant`;

/**
 * What the screen reader's live region says: that a reply is coming, then the
 * finished reply once. Only replies streamed in this view are read.
 */
const replyAnnouncement = (state: ChatState): string => {
  if (state.phase === "streaming") {
    return "Assistant is replying…";
  }
  if (state.phase === "comparing") {
    return "Models are answering…";
  }
  const last = state.messages.at(-1);
  if (last?.role !== "assistant" || last.live !== true) {
    return "";
  }
  if (last.status === "complete") {
    return `Assistant replied: ${last.content}`;
  }
  if (last.status === "cancelled") {
    return "Reply stopped.";
  }
  return last.status === "error" ? "Reply failed." : "";
};

/**
 * Ends the current stream, setting the assistant bubble's final status. A
 * regenerate or edit that did not complete shows what it replaced again,
 * as Rust has put it back.
 */
const finishStream = (
  state: ChatState,
  status: MessageStatus,
  changes: Partial<ChatState> = {}
): ChatState => {
  if (state.streamId === null) {
    return state;
  }
  const messages =
    state.revision !== null && status !== "complete"
      ? state.revision
      : updateMessage(state.messages, state.replyId, (message) => ({
          ...message,
          status,
        }));
  return {
    ...state,
    messages,
    phase: "idle",
    streamId: null,
    settledStreamId: state.streamId,
    replyId: null,
    requestId: null,
    revision: null,
    ...changes,
  };
};

const applyEvent = (
  state: ChatState,
  event: ChatEvent,
  at: number
): ChatState => {
  if (state.streamId === null) {
    return state;
  }
  const update = (change: (message: UiMessage) => UiMessage): ChatState => ({
    ...state,
    messages: updateMessage(state.messages, state.replyId, change),
  });
  if (event.type === "thinking" || event.type === "token") {
    return update((message) => withChunk(message, event, at));
  }
  if (event.type === "chart" || event.type === "chartError") {
    return update((message) => withChart(message, event));
  }
  if (event.type === "done") {
    return finishStream(
      update((message) => withStats(message, event)),
      "complete",
      { notice: event.doneReason === "length" ? "length" : null }
    );
  }
  return finishStream(state, "error", { error: event.error });
};

/**
 * Starts streaming into `messages`, which end with the new reply's bubble.
 * `revision`: what to show again if a regenerate or edit does not complete.
 */
const startStream = (
  state: ChatState,
  streamId: string,
  messages: UiMessage[],
  revision: UiMessage[] | null
): ChatState => ({
  ...state,
  messages,
  phase: "streaming",
  streamId,
  replyId: assistantId(streamId),
  requestId: null,
  revision,
  error: null,
  notice: null,
});

const replyBubble = (streamId: string, model?: string): UiMessage => ({
  id: assistantId(streamId),
  role: "assistant",
  content: "",
  status: "streaming",
  live: true,
  ...(model === undefined ? {} : { model }),
});

/**
 * Starts a turn, or refuses it while a reply streams, a comparison waits
 * for a pick, or messages load.
 */
const startTurn = (state: ChatState, action: TurnAction): ChatState => {
  if (state.phase !== "idle") {
    return { ...state, notice: "busy" };
  }
  if (state.compare !== null) {
    return { ...state, notice: "compare" };
  }
  // The load would replace the new bubbles when it lands.
  if (state.loading) {
    return state;
  }
  const { messages } = state;
  const { streamId, model } = action;
  if (action.type === "send") {
    return startStream(
      state,
      streamId,
      [
        ...messages,
        userBubble(streamId, action.text),
        replyBubble(streamId, model),
      ],
      null
    );
  }
  if (action.type === "regenerate") {
    return messages.at(-1)?.role === "assistant"
      ? startStream(
          state,
          streamId,
          [...messages.slice(0, -1), replyBubble(streamId, model)],
          messages
        )
      : state;
  }
  const edited = messages.findIndex(
    (message) => message.id === action.messageId && message.role === "user"
  );
  return edited === -1
    ? state
    : startStream(
        state,
        streamId,
        [
          ...messages.slice(0, edited),
          userBubble(streamId, action.text),
          replyBubble(streamId, model),
        ],
        messages
      );
};

/**
 * Rust never admitted the request, so nothing changed there. Drop the empty
 * bubble and mark the prompt failed, or show again what a regenerate or
 * edit replaced.
 */
const failSend = (
  state: ChatState,
  streamId: string,
  error: AppError
): ChatState => {
  const busy = error.kind === "busy";
  return {
    ...state,
    messages:
      state.revision ??
      updateMessage(
        state.messages.filter(
          (message) => message.id !== assistantId(streamId)
        ),
        userId(streamId),
        (message) => ({ ...message, status: "error" })
      ),
    phase: "idle",
    streamId: null,
    settledStreamId: streamId,
    replyId: null,
    requestId: null,
    revision: null,
    error: busy ? null : error,
    notice: busy ? "busy" : null,
  };
};

/**
 * Swaps a send's local ids for Rust's, so its messages can be edited later.
 * Applied even after an early Stop; the request id only for the live send.
 */
const adoptIds = (
  state: ChatState,
  streamId: string,
  { requestId, userMessageId, assistantMessageId }: SendStarted
): ChatState => {
  const local = userId(streamId);
  const reply = assistantId(streamId);
  const current = streamId === state.streamId;
  if (
    !current &&
    !state.messages.some(({ id }) => id === local || id === reply)
  ) {
    return state;
  }
  const messages = state.messages.map((message) => {
    if (message.id === local) {
      return { ...message, id: userMessageId, local: undefined };
    }
    return message.id === reply
      ? { ...message, id: assistantMessageId }
      : message;
  });
  return current
    ? { ...state, messages, replyId: assistantMessageId, requestId }
    : { ...state, messages };
};

/** Loads, refreshes, settings and deletes: not tied to one send. */
const applyChatChange = (
  state: ChatState,
  action: Extract<
    ChatAction,
    {
      type:
        | "loaded"
        | "loadFailed"
        | "refused"
        | "refreshed"
        | "settings"
        | "reloaded"
        | "deleted";
    }
  >
): ChatState => {
  if (action.type === "loaded" || action.type === "loadFailed") {
    // A slower load for a conversation that is no longer open is stale.
    if (action.conversationId !== state.conversationId || !state.loading) {
      return state;
    }
    if (action.type === "loadFailed") {
      return { ...state, loading: false, error: action.error };
    }
    const { messages, compare, settings } = action;
    return { ...state, loading: false, messages, compare, settings };
  }
  if (action.type === "refused") {
    return action.conversationId === state.conversationId
      ? { ...state, error: action.error }
      : state;
  }
  if (action.type === "settings") {
    // A new change replaces the last one's error; a failed save sets it.
    const { settings, error = null } = action;
    return { ...state, settings, error };
  }
  if (action.type === "deleted") {
    // Checked when the delete lands: another conversation may be open by now.
    return action.conversationId === state.conversationId &&
      state.phase === "idle"
      ? createChatState()
      : state;
  }
  // A reload is stale once another turn has started or ended since; a
  // refresh after a pick or discard only while idle.
  return action.conversationId === state.conversationId &&
    (action.type === "refreshed" ||
      action.streamId === state.settledStreamId) &&
    state.phase === "idle" &&
    !state.loading
    ? { ...state, messages: action.messages, compare: action.compare }
    : state;
};

const chatReducer = (state: ChatState, action: ChatAction): ChatState => {
  if (action.type === "open") {
    // Switching away would orphan the live reply's bubbles. A temporary chat
    // is discarded instead, and Rust cancels its reply.
    if (state.phase !== "idle" && !state.temporary) {
      return state;
    }
    return {
      ...createChatState(
        action.conversationId,
        false,
        action.projectId ?? null
      ),
      loading: action.conversationId !== null,
    };
  }
  if (action.type === "openTemporary") {
    return state.phase === "idle"
      ? createChatState(null, true, action.projectId ?? null)
      : state;
  }
  if (
    action.type === "loaded" ||
    action.type === "loadFailed" ||
    action.type === "refused" ||
    action.type === "refreshed" ||
    action.type === "settings" ||
    action.type === "reloaded" ||
    action.type === "deleted"
  ) {
    return applyChatChange(state, action);
  }
  if (
    action.type === "send" ||
    action.type === "regenerate" ||
    action.type === "edit"
  ) {
    return startTurn(state, action);
  }
  if (action.type === "compare") {
    return startCompare(state, action);
  }
  if (action.type === "cancel") {
    return state.phase === "comparing"
      ? finishCompare(state, true)
      : finishStream(state, "cancelled");
  }
  if (action.type === "created") {
    // Adopted even after an early Stop: the turn is saved there, and the
    // sidebar selects it. A view that moved on has an id of its own.
    return state.conversationId === null
      ? { ...state, conversationId: action.conversationId }
      : state;
  }
  if (action.type === "started") {
    return adoptIds(state, action.streamId, action.started);
  }
  // The rest belong to one send; results of an earlier send are stale.
  if (action.streamId !== state.streamId) {
    return state;
  }
  if (action.type === "event") {
    return applyEvent(state, action.event, action.at);
  }
  if (action.type === "compareStarted") {
    return adoptCompareIds(state, action.started);
  }
  if (action.type === "compareEvent") {
    return applyCompareEvent(state, action.event, action.at);
  }
  if (action.type === "compareFailed") {
    return failCompare(state, action.error);
  }
  return failSend(state, action.streamId, action.error);
};

export type {
  ChatState,
  CompareState,
  TurnAction,
} from "~/features/chat/chat-state";
export { createChatState } from "~/features/chat/chat-state";
export type { UiMessage } from "~/features/chat/ui-message";
export { chatReducer, replyAnnouncement };
