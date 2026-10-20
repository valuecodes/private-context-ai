import { useReducer, useRef } from "react";

import { toAppError } from "~/lib/app-error";
import { chatCancel, chatSend } from "~/lib/ipc";
import type { AppError, ChatEvent, ChatMessage, RequestId } from "~/lib/types";

type MessageStatus = "complete" | "streaming" | "cancelled" | "error";

type UiMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  status: MessageStatus;
};

/** `busy`: a send while a reply streams. `length`: the reply was cut off. */
type ChatNotice = "busy" | "length";

type ChatState = {
  /** Frontend-generated; Rust keeps no conversation of its own yet. */
  conversationId: string;
  messages: UiMessage[];
  phase: "idle" | "streaming";
  /** Local id of the current send; events for any other id are stale. */
  streamId: string | null;
  requestId: RequestId | null;
  error: AppError | null;
  notice: ChatNotice | null;
};

type ChatAction =
  | { type: "send"; streamId: string; text: string }
  | { type: "started"; streamId: string; requestId: RequestId }
  | { type: "event"; streamId: string; event: ChatEvent }
  | { type: "cancel" }
  | { type: "failed"; streamId: string; error: AppError };

const createChatState = (conversationId: string): ChatState => ({
  conversationId,
  messages: [],
  phase: "idle",
  streamId: null,
  requestId: null,
  error: null,
  notice: null,
});

const userId = (streamId: string): string => `${streamId}:user`;
const assistantId = (streamId: string): string => `${streamId}:assistant`;

const updateMessage = (
  messages: UiMessage[],
  id: string,
  update: (message: UiMessage) => UiMessage
): UiMessage[] =>
  messages.map((message) => (message.id === id ? update(message) : message));

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

const applyEvent = (state: ChatState, event: ChatEvent): ChatState => {
  if (state.streamId === null) {
    return state;
  }
  if (event.type === "token") {
    return {
      ...state,
      messages: updateMessage(
        state.messages,
        assistantId(state.streamId),
        (message) => ({
          ...message,
          content: message.content + event.content,
        })
      ),
    };
  }
  if (event.type === "done") {
    return finishStream(state, "complete", {
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
 * Rust never admitted the request. Drop the empty bubble and mark the prompt
 * failed, so it is not sent as history later.
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
  if (action.type === "send") {
    return startSend(state, action.streamId, action.text);
  }
  if (action.type === "cancel") {
    return finishStream(state, "cancelled");
  }
  // The rest belong to one send; results of an earlier send are stale.
  if (action.streamId !== state.streamId) {
    return state;
  }
  if (action.type === "started") {
    return { ...state, requestId: action.requestId };
  }
  if (action.type === "event") {
    return applyEvent(state, action.event);
  }
  return failSend(state, action.streamId, action.error);
};

/**
 * The turns sent to Ollama: completed ones, plus the visible part of a reply
 * that was stopped. Failed turns and empty bubbles are left out.
 */
const historyForRequest = (messages: UiMessage[]): ChatMessage[] =>
  messages
    .filter(
      (message) =>
        message.content !== "" &&
        (message.status === "complete" || message.status === "cancelled")
    )
    .map(({ role, content }) => ({ role, content }));

const newStreamId = (): string => crypto.randomUUID();

const useChatStream = () => {
  const [state, dispatch] = useReducer(chatReducer, undefined, (): ChatState =>
    createChatState(crypto.randomUUID())
  );
  // The latest send: its request id once Rust admits it, `null` if refused.
  // Stop can then cancel even before `chatSend` has resolved.
  const pending = useRef<{
    streamId: string;
    requestId: Promise<RequestId | null>;
  } | null>(null);
  // The latest cancel. A new send waits for it, and `chat_cancel` returns
  // only once Rust has freed the slot, so the resend is not refused as busy.
  const cancelling = useRef<Promise<void>>(Promise.resolve());

  /** Returns `false` when the send was refused because a reply streams. */
  const send = (model: string, text: string): boolean => {
    const streamId = newStreamId();
    dispatch({ type: "send", streamId, text });
    if (state.phase === "streaming") {
      return false;
    }
    const messages: ChatMessage[] = [
      ...historyForRequest(state.messages),
      { role: "user", content: text },
    ];
    const onEvent = (event: ChatEvent) => {
      dispatch({ type: "event", streamId, event });
    };
    const requestId = cancelling.current
      .then(() =>
        chatSend(
          { conversationId: state.conversationId, model, messages },
          onEvent
        )
      )
      .then(
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

  return { state, send, stop };
};

export type { ChatState, UiMessage };
export { chatReducer, createChatState, historyForRequest, useChatStream };
