import { useReducer, useRef } from "react";

import { startRequest, tempSession } from "~/features/chat/send-request";
import type {
  ChatCallbacks,
  SendTarget,
  TempSession,
} from "~/features/chat/send-request";
import { toAppError } from "~/lib/app-error";
import { chatCancel, conversationGet } from "~/lib/ipc";
import type {
  AppError,
  ChatEvent,
  RequestId,
  StoredMessage,
  Think,
} from "~/lib/types";

type MessageStatus = "complete" | "streaming" | "cancelled" | "error";

type UiMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  status: MessageStatus;
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
  requestId: RequestId | null;
  error: AppError | null;
  notice: ChatNotice | null;
};

type ChatAction =
  | { type: "open"; conversationId: string | null }
  | { type: "openTemporary" }
  | { type: "loaded"; conversationId: string; messages: UiMessage[] }
  | { type: "loadFailed"; conversationId: string; error: AppError }
  | { type: "send"; streamId: string; text: string }
  | { type: "created"; streamId: string; conversationId: string }
  | { type: "started"; streamId: string; requestId: RequestId }
  /** `at`: when the event arrived, in ms, so the reducer stays pure. */
  | { type: "event"; streamId: string; event: ChatEvent; at: number }
  | { type: "cancel" }
  | { type: "failed"; streamId: string; error: AppError }
  | { type: "deleted"; conversationId: string };

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
  requestId: null,
  error: null,
  notice: null,
});

const ASSISTANT_SUFFIX = ":assistant";

const userId = (streamId: string): string => `${streamId}:user`;
const assistantId = (streamId: string): string =>
  `${streamId}${ASSISTANT_SUFFIX}`;

/**
 * What the screen reader's live region says: that a reply is coming, then the
 * finished reply once. Only replies sent in this view are read: saved ones
 * carry database ids, not `assistantId`s.
 */
const replyAnnouncement = (state: ChatState): string => {
  if (state.phase === "streaming") {
    return "Assistant is replying…";
  }
  const last = state.messages.at(-1);
  if (last?.role !== "assistant" || !last.id.endsWith(ASSISTANT_SUFFIX)) {
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
  id: string,
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

/** Ends the current stream, setting the assistant bubble's final status. */
const finishStream = (
  state: ChatState,
  status: MessageStatus,
  changes: Partial<ChatState> = {}
): ChatState => {
  if (state.streamId === null) {
    return state;
  }
  return {
    ...state,
    messages: updateMessage(
      state.messages,
      assistantId(state.streamId),
      (message) => ({ ...message, status })
    ),
    phase: "idle",
    streamId: null,
    requestId: null,
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
  const { streamId } = state;
  if (streamId === null) {
    return state;
  }
  const update = (change: (message: UiMessage) => UiMessage): ChatState => ({
    ...state,
    messages: updateMessage(state.messages, assistantId(streamId), change),
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

const startSend = (
  state: ChatState,
  streamId: string,
  text: string
): ChatState => {
  if (state.phase === "streaming") {
    return { ...state, notice: "busy" };
  }
  // The load would replace the new bubbles when it lands.
  if (state.loading) {
    return state;
  }
  return {
    ...state,
    messages: [
      ...state.messages,
      { id: userId(streamId), role: "user", content: text, status: "complete" },
      {
        id: assistantId(streamId),
        role: "assistant",
        content: "",
        status: "streaming",
      },
    ],
    phase: "streaming",
    streamId,
    requestId: null,
    error: null,
    notice: null,
  };
};

/**
 * Rust never admitted the request, so nothing was saved. Drop the empty
 * bubble and mark the prompt failed.
 */
const failSend = (
  state: ChatState,
  streamId: string,
  error: AppError
): ChatState => {
  const busy = error.kind === "busy";
  return {
    ...state,
    messages: updateMessage(
      state.messages.filter((message) => message.id !== assistantId(streamId)),
      userId(streamId),
      (message) => ({ ...message, status: "error" })
    ),
    phase: "idle",
    streamId: null,
    requestId: null,
    error: busy ? null : error,
    notice: busy ? "busy" : null,
  };
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
  if (action.type === "deleted") {
    // Checked when the delete lands: another conversation may be open by now.
    return action.conversationId === state.conversationId &&
      state.phase === "idle"
      ? createChatState()
      : state;
  }
  if (action.type === "send") {
    return startSend(state, action.streamId, action.text);
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
  // The rest belong to one send; results of an earlier send are stale.
  if (action.streamId !== state.streamId) {
    return state;
  }
  if (action.type === "started") {
    return { ...state, requestId: action.requestId };
  }
  if (action.type === "event") {
    return applyEvent(state, action.event, action.at);
  }
  return failSend(state, action.streamId, action.error);
};

const newStreamId = (): string => crypto.randomUUID();

const useChatStream = ({ onCreated, onStarted }: ChatCallbacks) => {
  const [state, dispatch] = useReducer(chatReducer, null, createChatState);
  // One per chat view; `tempSession` itself starts nothing until a send.
  const session = useRef<TempSession>(tempSession()).current;
  // The latest send: its request id once Rust admits it, `null` if refused.
  // Stop can then cancel even before `chatSend` has resolved.
  const pending = useRef<{
    streamId: string;
    requestId: Promise<RequestId | null>;
  } | null>(null);
  // The latest cancel. A new send waits for it, and `chat_cancel` returns
  // only once Rust has freed the slot, so the resend is not refused as busy.
  const cancelling = useRef<Promise<void>>(Promise.resolve());

  /**
   * Shows a saved conversation, or a new chat for `null`. Leaving a
   * temporary chat discards it, even while it answers.
   */
  const open = (conversationId: string | null) => {
    if (state.phase === "streaming" && !state.temporary) {
      return;
    }
    session.discard();
    dispatch({ type: "open", conversationId });
    if (conversationId === null) {
      return;
    }
    void conversationGet(conversationId).then(
      (saved) => {
        dispatch({
          type: "loaded",
          conversationId,
          messages: fromStored(saved.messages),
        });
      },
      (error: unknown) => {
        dispatch({
          type: "loadFailed",
          conversationId,
          error: toAppError(error),
        });
      }
    );
  };

  /** Starts an empty temporary chat; its session starts with the first send. */
  const openTemporary = () => {
    if (state.phase === "streaming") {
      return;
    }
    session.discard();
    dispatch({ type: "openTemporary" });
  };

  /** Clears the view if the deleted conversation is still the open one. */
  const forget = (conversationId: string) => {
    dispatch({ type: "deleted", conversationId });
  };

  /**
   * Returns `false` when the send was refused because a reply streams or the
   * conversation is still loading.
   */
  const send = (model: string, text: string, think: Think | null): boolean => {
    const streamId = newStreamId();
    dispatch({ type: "send", streamId, text });
    if (state.phase === "streaming" || state.loading) {
      return false;
    }
    const onEvent = (event: ChatEvent) => {
      dispatch({ type: "event", streamId, event, at: Date.now() });
    };
    const saved = state.conversationId;
    let target: SendTarget = { kind: "new" };
    if (state.temporary) {
      target = { kind: "temporary", sessionId: session.id() };
    } else if (saved !== null) {
      target = { kind: "saved", conversationId: saved };
    }
    const requestId = startRequest(
      target,
      { model, prompt: text, think },
      onEvent,
      {
        onCreated: (created) => {
          dispatch({ type: "created", streamId, conversationId: created.id });
          onCreated(created);
        },
        onStarted,
      },
      cancelling.current
    ).then(
      (id) => {
        dispatch({ type: "started", streamId, requestId: id });
        return id;
      },
      (error: unknown) => {
        dispatch({ type: "failed", streamId, error: toAppError(error) });
        return null;
      }
    );
    pending.current = { streamId, requestId };
    return true;
  };

  const stop = () => {
    const current = pending.current;
    if (
      state.phase !== "streaming" ||
      current === null ||
      current.streamId !== state.streamId
    ) {
      return;
    }
    cancelling.current = current.requestId
      .then((id) => (id === null ? undefined : chatCancel(id)))
      .catch(() => undefined);
    dispatch({ type: "cancel" });
  };

  return { state, open, openTemporary, forget, send, stop };
};

export type { ChatState, UiMessage };
export {
  chatReducer,
  createChatState,
  fromStored,
  replyAnnouncement,
  useChatStream,
};
