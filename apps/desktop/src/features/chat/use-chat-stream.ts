import { useReducer, useRef } from "react";

import {
  chatReducer,
  createChatState,
  fromStored,
} from "~/features/chat/chat-reducer";
import type { TurnAction, UiMessage } from "~/features/chat/chat-reducer";
import { startRequest, tempSession } from "~/features/chat/send-request";
import type {
  ChatCallbacks,
  SendTarget,
  TempSession,
  TurnInput,
} from "~/features/chat/send-request";
import { toAppError } from "~/lib/app-error";
import { chatCancel, conversationGet } from "~/lib/ipc";
import type { AppError, ChatEvent, RequestId, Think } from "~/lib/types";

const newStreamId = (): string => crypto.randomUUID();

/** Loads a saved conversation's messages as bubbles. */
const loadConversation = (
  conversationId: string,
  loaded: (messages: UiMessage[]) => void,
  failed: (error: AppError) => void
) => {
  void conversationGet(conversationId).then(
    (saved) => {
      loaded(fromStored(saved.messages));
    },
    (error: unknown) => {
      failed(toAppError(error));
    }
  );
};

const useChatStream = ({ onCreated, onStarted }: ChatCallbacks) => {
  const [state, dispatch] = useReducer(chatReducer, null, createChatState);
  // One per chat view; `tempSession` itself starts nothing until a send.
  const session = useRef<TempSession>(tempSession()).current;
  // The latest send: its request id once Rust admits it, `null` if refused.
  // Stop can then cancel even before `chatSend` has resolved.
  const pending = useRef<{
    streamId: string;
    requestId: Promise<RequestId | null>;
    /** Shows the saved conversation again, for a revision that ended. */
    reload: () => void;
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
    loadConversation(
      conversationId,
      (messages) => {
        dispatch({ type: "loaded", conversationId, messages });
      },
      (error) => {
        dispatch({ type: "loadFailed", conversationId, error });
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
   * Starts `action` and its request. Returns `false` when it was refused
   * because a reply streams, the conversation is still loading, or there is
   * nothing to regenerate or edit.
   */
  const startTurnRequest = (
    action: TurnAction,
    model: string,
    think: Think | null,
    input: TurnInput
  ): boolean => {
    const { streamId } = action;
    const accepted = chatReducer(state, action).streamId === streamId;
    dispatch(action);
    if (!accepted) {
      return false;
    }
    const saved = state.conversationId;
    // A refused regenerate or edit was undone in Rust before the refusal
    // arrived, and a stopped one may have completed there first: show the
    // saved conversation as it really is. A reply still pending there (the
    // cancel stopped waiting) keeps what the view restored.
    const reload = () => {
      if (input.kind !== "send" && !state.temporary && saved !== null) {
        loadConversation(
          saved,
          (messages) => {
            if (messages.every(({ status }) => status !== "streaming")) {
              dispatch({
                type: "reloaded",
                conversationId: saved,
                streamId,
                messages,
              });
            }
          },
          () => undefined
        );
      }
    };
    const onEvent = (event: ChatEvent) => {
      dispatch({ type: "event", streamId, event, at: Date.now() });
      if (event.type === "error") {
        reload();
      }
    };
    let target: SendTarget = { kind: "new" };
    if (state.temporary) {
      target = { kind: "temporary", sessionId: session.id() };
    } else if (saved !== null) {
      target = { kind: "saved", conversationId: saved };
    }
    const requestId = startRequest(
      target,
      { model, think, input },
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
      (started) => {
        dispatch({ type: "started", streamId, started });
        return started.requestId;
      },
      (error: unknown) => {
        dispatch({ type: "failed", streamId, error: toAppError(error) });
        reload();
        return null;
      }
    );
    pending.current = { streamId, requestId, reload };
    return true;
  };

  const send = (model: string, text: string, think: Think | null): boolean =>
    startTurnRequest(
      { type: "send", streamId: newStreamId(), text },
      model,
      think,
      { kind: "send", prompt: text }
    );

  /** A new reply to the last prompt, replacing the last reply. */
  const regenerate = (model: string, think: Think | null): boolean =>
    startTurnRequest(
      { type: "regenerate", streamId: newStreamId() },
      model,
      think,
      { kind: "regenerate" }
    );

  /** Replaces the user message `messageId`, and everything after it. */
  const edit = (
    model: string,
    messageId: string,
    text: string,
    think: Think | null
  ): boolean =>
    startTurnRequest(
      { type: "edit", streamId: newStreamId(), messageId, text },
      model,
      think,
      { kind: "edit", messageId, prompt: text }
    );

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
    // Once Rust has ended the reply: it may have completed before the Stop.
    void cancelling.current.then(current.reload);
    dispatch({ type: "cancel" });
  };

  return {
    state,
    open,
    openTemporary,
    forget,
    send,
    regenerate,
    edit,
    stop,
  };
};

export { useChatStream };
