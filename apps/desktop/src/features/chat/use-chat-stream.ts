import { useReducer, useRef } from "react";

import { chatReducer, createChatState } from "~/features/chat/chat-reducer";
import type {
  CompareState,
  TurnAction,
  UiMessage,
} from "~/features/chat/chat-reducer";
import { startRequest, tempSession } from "~/features/chat/send-request";
import type {
  ChatCallbacks,
  SendTarget,
  TempSession,
  TurnInput,
} from "~/features/chat/send-request";
import { settingsSync } from "~/features/chat/settings-sync";
import type { SettingsSync } from "~/features/chat/settings-sync";
import { splitStored } from "~/features/chat/stored-messages";
import { toAppError } from "~/lib/app-error";
import {
  chatCancel,
  chatCompare,
  chatCompareDiscard,
  chatComparePick,
  conversationGet,
} from "~/lib/ipc";
import type {
  AppError,
  ChatEvent,
  CompareEvent,
  ConversationSettings,
  RequestId,
} from "~/lib/types";

const newStreamId = (): string => crypto.randomUUID();

type Loaded = (
  messages: UiMessage[],
  compare: CompareState | null,
  settings: ConversationSettings
) => void;

/**
 * Loads a saved conversation's messages as bubbles, its comparison waiting
 * for a pick, and its settings.
 */
const loadConversation = (
  conversationId: string,
  loaded: Loaded,
  failed: (error: AppError) => void
) => {
  void conversationGet(conversationId).then(
    (saved) => {
      const { messages, compare } = splitStored(saved.messages);
      loaded(messages, compare, saved.conversation.settings);
    },
    (error: unknown) => {
      failed(toAppError(error));
    }
  );
};

/**
 * Holds pushed items until `release`, then delivers them in order with the
 * time each arrived; later items are delivered at once.
 */
const holdUntilReleased = <T>(deliver: (item: T, at: number) => void) => {
  let held: { item: T; at: number }[] | null = [];
  return {
    push: (item: T) => {
      if (held === null) {
        deliver(item, Date.now());
      } else {
        held.push({ item, at: Date.now() });
      }
    },
    release: () => {
      const waiting = held ?? [];
      held = null;
      for (const { item, at } of waiting) {
        deliver(item, at);
      }
    },
  };
};

/** Nothing in the conversation or its comparison is still answering. */
const settled = (messages: UiMessage[], compare: CompareState | null) =>
  [...messages, ...(compare?.candidates ?? [])].every(
    ({ status }) => status !== "streaming"
  );

const useChatStream = ({ onCreated, onStarted }: ChatCallbacks) => {
  const [state, dispatch] = useReducer(chatReducer, null, createChatState);
  // One per chat view; `tempSession` itself starts nothing until a send.
  const session = useRef<TempSession>(tempSession()).current;
  const sync = useRef<SettingsSync>(settingsSync()).current;
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
   * Shows a saved conversation, or a new chat for `null`, in project
   * `projectId` unless it is `null`. Leaving a temporary chat discards it,
   * even while it answers.
   */
  const open = (conversationId: string | null, projectId: string | null) => {
    if (state.phase !== "idle" && !state.temporary) {
      return;
    }
    session.discard();
    const opened = sync.open(conversationId);
    dispatch({ type: "open", conversationId, projectId });
    if (conversationId === null) {
      return;
    }
    // After the saves still queued, which may be for this very chat, so the
    // load reads what they wrote. The view stays loading meanwhile.
    void sync.settled().then(() => {
      loadConversation(
        conversationId,
        (messages, compare, settings) => {
          opened.saved = settings;
          dispatch({
            type: "loaded",
            conversationId,
            messages,
            compare,
            settings,
          });
        },
        (error) => {
          dispatch({ type: "loadFailed", conversationId, error });
        }
      );
    });
  };

  /**
   * Starts an empty temporary chat, in project `projectId` unless it is
   * `null`; its session starts with the first send.
   */
  const openTemporary = (projectId: string | null) => {
    if (state.phase !== "idle") {
      return;
    }
    session.discard();
    sync.open(null);
    dispatch({ type: "openTemporary", projectId });
  };

  /** Clears the view if the deleted conversation is still the open one. */
  const forget = (conversationId: string) => {
    // The reducer's condition: the view resets only when idle.
    if (
      sync.current().conversationId === conversationId &&
      state.phase === "idle"
    ) {
      sync.open(null);
    }
    dispatch({ type: "deleted", conversationId });
  };

  /**
   * Shows `settings` at once and saves them for the open chat. A failed
   * save shows the error and the settings Rust kept.
   */
  const updateSettings = (settings: ConversationSettings) => {
    if (state.loading) {
      return;
    }
    dispatch({ type: "settings", settings });
    sync.save(settings, (saved, error) => {
      dispatch({ type: "settings", settings: saved, error });
    });
  };

  /** Shows the saved conversation once `streamId` has ended there. */
  const reloadAfter = (conversationId: string, streamId: string) => {
    loadConversation(
      conversationId,
      (messages, compare) => {
        if (settled(messages, compare)) {
          dispatch({
            type: "reloaded",
            conversationId,
            streamId,
            messages,
            compare,
          });
        }
      },
      () => undefined
    );
  };

  /**
   * Starts `action` and its request. Returns `false` when it was refused
   * because a reply streams, the conversation is still loading, or there is
   * nothing to regenerate or edit.
   */
  const startTurnRequest = (
    action: TurnAction,
    model: string,
    input: TurnInput
  ): boolean => {
    const { streamId } = action;
    const accepted = chatReducer(state, action).streamId === streamId;
    dispatch(action);
    if (!accepted) {
      return false;
    }
    const saved = state.conversationId;
    const { settings } = state;
    const current = sync.current();
    // A refused regenerate or edit was undone in Rust before the refusal
    // arrived, and a stopped one may have completed there first: show the
    // saved conversation as it really is. A reply still pending there (the
    // cancel stopped waiting) keeps what the view restored.
    const reload = () => {
      if (input.kind !== "send" && !state.temporary && saved !== null) {
        reloadAfter(saved, streamId);
      }
    };
    const onEvent = (event: ChatEvent) => {
      dispatch({ type: "event", streamId, event, at: Date.now() });
      if (event.type === "error") {
        reload();
      }
    };
    let target: SendTarget = { kind: "new", projectId: state.projectId };
    if (state.temporary) {
      // The same session every send; a refused start is tried again.
      current.session = session.id(settings, state.projectId);
      target = { kind: "temporary", sessionId: current.session };
    } else if (saved !== null) {
      target = { kind: "saved", conversationId: saved };
    }
    // A send waits for the settings saved before it, and for a Stop.
    const ready = Promise.all([cancelling.current, sync.settled()]).then(
      () => undefined
    );
    const requestId = startRequest(
      target,
      { model, settings, input },
      onEvent,
      {
        onCreated: (created) => {
          current.conversationId = created.id;
          current.saved = created.settings;
          dispatch({ type: "created", streamId, conversationId: created.id });
          onCreated(created);
        },
        onStarted,
      },
      ready
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
    current.started = requestId;
    pending.current = { streamId, requestId, reload };
    return true;
  };

  const send = (model: string, text: string): boolean =>
    startTurnRequest(
      { type: "send", streamId: newStreamId(), text, model },
      model,
      { kind: "send", prompt: text }
    );

  /** A new reply to the last prompt, replacing the last reply. */
  const regenerate = (model: string): boolean =>
    startTurnRequest(
      { type: "regenerate", streamId: newStreamId(), model },
      model,
      { kind: "regenerate" }
    );

  /** Replaces the user message `messageId`, and everything after it. */
  const edit = (model: string, messageId: string, text: string): boolean =>
    startTurnRequest(
      { type: "edit", streamId: newStreamId(), messageId, text, model },
      model,
      { kind: "edit", messageId, prompt: text }
    );

  /**
   * Asks `models` the prompt `text` one after another, in a saved chat.
   * Returns `false` when it was refused.
   */
  const compare = (models: string[], text: string): boolean => {
    const streamId = newStreamId();
    const action = { type: "compare", streamId, text, models } as const;
    const conversationId = state.conversationId;
    const accepted = chatReducer(state, action).streamId === streamId;
    dispatch(action);
    if (!accepted || conversationId === null) {
      return false;
    }
    const reload = () => {
      reloadAfter(conversationId, streamId);
    };
    const deliver = (event: CompareEvent, at: number) => {
      dispatch({ type: "compareEvent", streamId, event, at });
      // Also after a Stop: the saved statuses replace the view's guess.
      if (event.type === "finished") {
        reload();
      }
    };
    // Events can arrive before `chatCompare` resolves, while the candidates
    // still have local ids: they wait until Rust's ids are adopted.
    const events = holdUntilReleased(deliver);
    const ready = Promise.all([cancelling.current, sync.settled()]);
    const requestId = ready
      .then(() =>
        chatCompare({ conversationId, prompt: text, models }, events.push)
      )
      .then(
        (started) => {
          dispatch({ type: "compareStarted", streamId, started });
          events.release();
          return started.requestId;
        },
        (error: unknown) => {
          dispatch({
            type: "compareFailed",
            streamId,
            error: toAppError(error),
          });
          return null;
        }
      );
    // A settings change waits for it, as for a send.
    sync.current().started = requestId;
    pending.current = { streamId, requestId, reload };
    return true;
  };

  /** Shows the saved conversation after a pick or discard. */
  const refresh = (conversationId: string) => {
    loadConversation(
      conversationId,
      (messages, compared) => {
        dispatch({
          type: "refreshed",
          conversationId,
          messages,
          compare: compared,
        });
      },
      () => undefined
    );
  };

  /**
   * Keeps the candidate `messageId`; the chat continues with its model.
   * Resolves `false` when it was refused.
   */
  const pick = (messageId: string): Promise<boolean> => {
    const conversationId = state.conversationId;
    const candidate = state.compare?.candidates.find(
      ({ id }) => id === messageId
    );
    if (
      conversationId === null ||
      state.phase !== "idle" ||
      candidate?.status !== "complete"
    ) {
      return Promise.resolve(false);
    }
    return chatComparePick(conversationId, messageId).then(
      () => {
        refresh(conversationId);
        if (candidate.model !== undefined) {
          onStarted(conversationId, candidate.model);
        }
        return true;
      },
      (error: unknown) => {
        dispatch({ type: "refused", conversationId, error: toAppError(error) });
        refresh(conversationId);
        return false;
      }
    );
  };

  /** Deletes the compared prompt and every candidate. */
  const discard = () => {
    const conversationId = state.conversationId;
    const group = state.compare?.group ?? null;
    if (conversationId === null || group === null || state.phase !== "idle") {
      return;
    }
    void chatCompareDiscard(conversationId, group).then(
      () => {
        refresh(conversationId);
      },
      (error: unknown) => {
        dispatch({ type: "refused", conversationId, error: toAppError(error) });
        refresh(conversationId);
      }
    );
  };

  const stop = () => {
    const current = pending.current;
    if (
      state.phase === "idle" ||
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
    updateSettings,
    send,
    regenerate,
    edit,
    compare,
    pick,
    discard,
    stop,
  };
};

export { holdUntilReleased, useChatStream };
