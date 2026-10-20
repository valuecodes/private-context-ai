import { useReducer, useRef } from "react";

import { toAppError } from "~/lib/app-error";
import {
  chatCancel,
  chatSend,
  conversationCreate,
  conversationGet,
} from "~/lib/ipc";
import type {
  AppError,
  ChatEvent,
  Conversation,
  RequestId,
  StoredMessage,
} from "~/lib/types";

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
  /** The open conversation; `null` for a new chat the first send saves. */
  conversationId: string | null;
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
  | { type: "loaded"; conversationId: string; messages: UiMessage[] }
  | { type: "loadFailed"; conversationId: string; error: AppError }
  | { type: "send"; streamId: string; text: string }
  | { type: "created"; streamId: string; conversationId: string }
  | { type: "started"; streamId: string; requestId: RequestId }
  | { type: "event"; streamId: string; event: ChatEvent }
  | { type: "cancel" }
  | { type: "failed"; streamId: string; error: AppError }
  | { type: "deleted"; conversationId: string };

const createChatState = (conversationId: string | null = null): ChatState => ({
  conversationId,
  loading: false,
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

/** Saved messages as bubbles. A `pending` row shows as still streaming. */
const fromStored = (messages: StoredMessage[]): UiMessage[] =>
  messages.flatMap((message) =>
    message.role === "system"
      ? []
      : [
          {
            id: message.id,
            role: message.role,
            content: message.content,
            status: message.status === "pending" ? "streaming" : message.status,
          },
        ]
  );

const TITLE_CHARS = 60;

/**
 * A title for a new conversation: the prompt's first non-empty line, cut to
 * 60 characters. `null` lets Rust use its default.
 */
const titleFromPrompt = (prompt: string): string | null => {
  const line = prompt
    .split("\n")
    .map((part) => part.replaceAll(/\p{Cc}/gu, " ").trim())
    .find((part) => part !== "");
  if (line === undefined) {
    return null;
  }
  // Spread splits by code point, so a cut never halves an emoji.
  return [...line].slice(0, TITLE_CHARS).join("").trim();
};

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
    // Switching away would orphan the live reply's bubbles.
    if (state.phase === "streaming") {
      return state;
    }
    return {
      ...createChatState(action.conversationId),
      loading: action.conversationId !== null,
    };
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
    return applyEvent(state, action.event);
  }
  return failSend(state, action.streamId, action.error);
};

const newStreamId = (): string => crypto.randomUUID();

type ChatCallbacks = {
  /** The first send of a new chat saved it as this conversation. */
  onCreated: (conversation: Conversation) => void;
  /** Rust admitted a send, so the conversation was just used with `model`. */
  onStarted: (conversationId: string, model: string) => void;
};

const useChatStream = ({ onCreated, onStarted }: ChatCallbacks) => {
  const [state, dispatch] = useReducer(chatReducer, null, createChatState);
  // The latest send: its request id once Rust admits it, `null` if refused.
  // Stop can then cancel even before `chatSend` has resolved.
  const pending = useRef<{
    streamId: string;
    requestId: Promise<RequestId | null>;
  } | null>(null);
  // The latest cancel. A new send waits for it, and `chat_cancel` returns
  // only once Rust has freed the slot, so the resend is not refused as busy.
  const cancelling = useRef<Promise<void>>(Promise.resolve());

  /** Shows a saved conversation, or a new chat for `null`. */
  const open = (conversationId: string | null) => {
    if (state.phase === "streaming") {
      return;
    }
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

  /** Clears the view if the deleted conversation is still the open one. */
  const forget = (conversationId: string) => {
    dispatch({ type: "deleted", conversationId });
  };

  /**
   * Returns `false` when the send was refused because a reply streams or the
   * conversation is still loading.
   */
  const send = (model: string, text: string): boolean => {
    const streamId = newStreamId();
    dispatch({ type: "send", streamId, text });
    if (state.phase === "streaming" || state.loading) {
      return false;
    }
    const onEvent = (event: ChatEvent) => {
      dispatch({ type: "event", streamId, event });
    };
    const saved = state.conversationId;
    const conversationId: Promise<string> =
      saved === null
        ? conversationCreate(titleFromPrompt(text), model).then((created) => {
            dispatch({ type: "created", streamId, conversationId: created.id });
            onCreated(created);
            return created.id;
          })
        : Promise.resolve(saved);
    const requestId = cancelling.current
      .then(() => conversationId)
      .then((id) =>
        chatSend({ conversationId: id, model, prompt: text }, onEvent).then(
          (request) => {
            onStarted(id, model);
            return request;
          }
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

  return { state, open, forget, send, stop };
};

export type { ChatState, UiMessage };
export {
  chatReducer,
  createChatState,
  fromStored,
  titleFromPrompt,
  useChatStream,
};
