import type {
  AppError,
  ChatEvent,
  RequestId,
  SendStarted,
  StoredMessage,
} from "~/lib/types";

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
};

/** `busy`: a send while a reply streams. `length`: the reply was cut off. */
type ChatNotice = "busy" | "length";

type ChatState = {
  /** The open conversation; `null` for a new chat the first send saves. */
  conversationId: string | null;
  /** A temporary chat: kept in Rust's memory only, never saved. */
  temporary: boolean;
  /** The saved messages are still loading; sending waits for them. */
  loading: boolean;
  messages: UiMessage[];
  phase: "idle" | "streaming";
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
};

type ChatAction =
  | { type: "open"; conversationId: string | null }
  | { type: "openTemporary" }
  | { type: "loaded"; conversationId: string; messages: UiMessage[] }
  | { type: "loadFailed"; conversationId: string; error: AppError }
  /**
   * The saved messages again, after the regenerate or edit `streamId` was
   * refused or stopped.
   */
  | {
      type: "reloaded";
      conversationId: string;
      streamId: string;
      messages: UiMessage[];
    }
  | { type: "send"; streamId: string; text: string }
  | { type: "regenerate"; streamId: string }
  | { type: "edit"; streamId: string; messageId: string; text: string }
  | { type: "created"; streamId: string; conversationId: string }
  | { type: "started"; streamId: string; started: SendStarted }
  /** `at`: when the event arrived, in ms, so the reducer stays pure. */
  | { type: "event"; streamId: string; event: ChatEvent; at: number }
  | { type: "cancel" }
  | { type: "failed"; streamId: string; error: AppError }
  | { type: "deleted"; conversationId: string };

type TurnAction = Extract<
  ChatAction,
  { type: "send" } | { type: "regenerate" } | { type: "edit" }
>;

const createChatState = (
  conversationId: string | null = null,
  temporary = false
): ChatState => ({
  conversationId,
  temporary,
  loading: false,
  messages: [],
  phase: "idle",
  streamId: null,
  settledStreamId: null,
  replyId: null,
  requestId: null,
  revision: null,
  error: null,
  notice: null,
});

const userId = (streamId: string): string => `${streamId}:user`;
const assistantId = (streamId: string): string => `${streamId}:assistant`;

/**
 * What the screen reader's live region says: that a reply is coming, then the
 * finished reply once. Only replies streamed in this view are read.
 */
const replyAnnouncement = (state: ChatState): string => {
  if (state.phase === "streaming") {
    return "Assistant is replying…";
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

const updateMessage = (
  messages: UiMessage[],
  id: string | null,
  update: (message: UiMessage) => UiMessage
): UiMessage[] =>
  messages.map((message) => (message.id === id ? update(message) : message));

/** Saved messages as bubbles. A `pending` row shows as still streaming. */
const fromStored = (messages: StoredMessage[]): UiMessage[] =>
  messages.flatMap((message): UiMessage[] => {
    if (message.role === "system") {
      return [];
    }
    const bubble: UiMessage = {
      id: message.id,
      role: message.role,
      content: message.content,
      status: message.status === "pending" ? "streaming" : message.status,
    };
    return message.thinking === null || message.thinking === ""
      ? [bubble]
      : [
          {
            ...bubble,
            thinking: message.thinking,
            thinkingMs: message.thinkingMs,
          },
        ];
  });

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

/** The thinking time once the answer starts, if it was not known yet. */
const thoughtFor = (message: UiMessage, at: number): number | null => {
  if (message.thinkingMs !== undefined && message.thinkingMs !== null) {
    return message.thinkingMs;
  }
  return message.thinkingStartedAt === undefined
    ? null
    : at - message.thinkingStartedAt;
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
  if (event.type === "thinking") {
    return update((message) => ({
      ...message,
      thinking: (message.thinking ?? "") + event.content,
      thinkingStartedAt: message.thinkingStartedAt ?? at,
    }));
  }
  if (event.type === "token") {
    return update((message) => ({
      ...message,
      content: message.content + event.content,
      ...(message.thinking === undefined
        ? {}
        : { thinkingMs: thoughtFor(message, at) }),
    }));
  }
  if (event.type === "done") {
    const finished =
      event.thinkingMs === null
        ? state
        : update((message) => ({ ...message, thinkingMs: event.thinkingMs }));
    return finishStream(finished, "complete", {
      notice: event.doneReason === "length" ? "length" : null,
    });
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

const userBubble = (streamId: string, text: string): UiMessage => ({
  id: userId(streamId),
  role: "user",
  content: text,
  status: "complete",
  local: true,
});

const replyBubble = (streamId: string): UiMessage => ({
  id: assistantId(streamId),
  role: "assistant",
  content: "",
  status: "streaming",
  live: true,
});

/** Starts a turn, or refuses it while a reply streams or messages load. */
const startTurn = (state: ChatState, action: TurnAction): ChatState => {
  if (state.phase === "streaming") {
    return { ...state, notice: "busy" };
  }
  // The load would replace the new bubbles when it lands.
  if (state.loading) {
    return state;
  }
  const { messages } = state;
  const { streamId } = action;
  if (action.type === "send") {
    return startStream(
      state,
      streamId,
      [...messages, userBubble(streamId, action.text), replyBubble(streamId)],
      null
    );
  }
  if (action.type === "regenerate") {
    return messages.at(-1)?.role === "assistant"
      ? startStream(
          state,
          streamId,
          [...messages.slice(0, -1), replyBubble(streamId)],
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
          replyBubble(streamId),
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

const chatReducer = (state: ChatState, action: ChatAction): ChatState => {
  if (action.type === "open") {
    // Switching away would orphan the live reply's bubbles. A temporary chat
    // is discarded instead, and Rust cancels its reply.
    if (state.phase === "streaming" && !state.temporary) {
      return state;
    }
    return {
      ...createChatState(action.conversationId),
      loading: action.conversationId !== null,
    };
  }
  if (action.type === "openTemporary") {
    return state.phase === "streaming" ? state : createChatState(null, true);
  }
  if (action.type === "loaded" || action.type === "loadFailed") {
    // A slower load for a conversation that is no longer open is stale.
    if (action.conversationId !== state.conversationId || !state.loading) {
      return state;
    }
    return action.type === "loaded"
      ? { ...state, loading: false, messages: action.messages }
      : { ...state, loading: false, error: action.error };
  }
  if (action.type === "reloaded") {
    // Stale once another turn has started or ended since.
    return action.conversationId === state.conversationId &&
      action.streamId === state.settledStreamId &&
      state.phase === "idle" &&
      !state.loading
      ? { ...state, messages: action.messages }
      : state;
  }
  if (action.type === "deleted") {
    // Checked when the delete lands: another conversation may be open by now.
    return action.conversationId === state.conversationId &&
      state.phase === "idle"
      ? createChatState()
      : state;
  }
  if (
    action.type === "send" ||
    action.type === "regenerate" ||
    action.type === "edit"
  ) {
    return startTurn(state, action);
  }
  if (action.type === "cancel") {
    return finishStream(state, "cancelled");
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
  return failSend(state, action.streamId, action.error);
};

export type { ChatState, TurnAction, UiMessage };
export { chatReducer, createChatState, fromStored, replyAnnouncement };
