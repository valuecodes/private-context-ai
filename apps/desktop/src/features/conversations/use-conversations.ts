import { useEffect, useReducer } from "react";

import { toAppError } from "~/lib/app-error";
import { copyLater } from "~/lib/clipboard";
import {
  conversationDelete,
  conversationList,
  conversationMarkdown,
  conversationRename,
} from "~/lib/ipc";
import type { AppError, Conversation } from "~/lib/types";

const COPY_FAILED: AppError = {
  kind: "internal",
  message: "Could not copy to the clipboard",
};

type ConversationsState = {
  phase: "loading" | "ready";
  /** Most recently used first. */
  conversations: Conversation[];
  /** `null`: a new chat that is not saved yet. */
  selectedId: string | null;
  error: AppError | null;
};

type ConversationsAction =
  | { type: "loaded"; conversations: Conversation[] }
  | { type: "selected"; id: string | null }
  | { type: "created"; conversation: Conversation }
  | { type: "renamed"; conversation: Conversation }
  | { type: "deleted"; id: string }
  | { type: "touched"; id: string; model: string }
  | { type: "failed"; error: AppError };

const initialConversations: ConversationsState = {
  phase: "loading",
  conversations: [],
  selectedId: null,
  error: null,
};

const moveToTop = (
  conversations: Conversation[],
  id: string
): Conversation[] => {
  const moved = conversations.find((conversation) => conversation.id === id);
  return moved === undefined
    ? conversations
    : [
        moved,
        ...conversations.filter((conversation) => conversation !== moved),
      ];
};

const conversationsReducer = (
  state: ConversationsState,
  action: ConversationsAction
): ConversationsState => {
  if (action.type === "loaded") {
    // Keep chats created while the list was loading; the query may predate them.
    const created = state.conversations.filter(
      (local) => !action.conversations.some(({ id }) => id === local.id)
    );
    return {
      ...state,
      phase: "ready",
      conversations: [...created, ...action.conversations],
      error: null,
    };
  }
  if (action.type === "selected") {
    return { ...state, selectedId: action.id, error: null };
  }
  if (action.type === "created") {
    return {
      ...state,
      conversations: [action.conversation, ...state.conversations],
      selectedId: action.conversation.id,
      error: null,
    };
  }
  if (action.type === "renamed") {
    return {
      ...state,
      conversations: state.conversations.map((conversation) =>
        conversation.id === action.conversation.id
          ? action.conversation
          : conversation
      ),
      error: null,
    };
  }
  if (action.type === "deleted") {
    return {
      ...state,
      conversations: state.conversations.filter(
        (conversation) => conversation.id !== action.id
      ),
      selectedId: state.selectedId === action.id ? null : state.selectedId,
      error: null,
    };
  }
  if (action.type === "touched") {
    // Rust saved the model with the send; keep the cached copy in step.
    const updated = state.conversations.map((conversation) =>
      conversation.id === action.id
        ? { ...conversation, model: action.model }
        : conversation
    );
    return { ...state, conversations: moveToTop(updated, action.id) };
  }
  return { ...state, phase: "ready", error: action.error };
};

/**
 * Copies a conversation as Markdown; resolves `null` once copied, else why
 * not. Call it straight from the click: the clipboard write starts before
 * Rust answers.
 */
const copyConversationMarkdown = (id: string): Promise<AppError | null> => {
  let fetchError: AppError | null = null;
  const markdown = conversationMarkdown(id).catch((error: unknown) => {
    fetchError = toAppError(error);
    throw error;
  });
  return copyLater(markdown).then((copied) =>
    copied ? null : (fetchError ?? COPY_FAILED)
  );
};

/** The saved conversations and which one is open. */
const useConversations = () => {
  const [state, dispatch] = useReducer(
    conversationsReducer,
    initialConversations
  );

  useEffect(() => {
    let cancelled = false;
    void conversationList().then(
      (conversations) => {
        if (!cancelled) {
          dispatch({ type: "loaded", conversations });
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          dispatch({ type: "failed", error: toAppError(error) });
        }
      }
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const select = (id: string | null) => {
    dispatch({ type: "selected", id });
  };

  const created = (conversation: Conversation) => {
    dispatch({ type: "created", conversation });
  };

  const touched = (id: string, model: string) => {
    dispatch({ type: "touched", id, model });
  };

  /** Resolves `false` when Rust refused the title. */
  const rename = (id: string, title: string): Promise<boolean> =>
    conversationRename(id, title).then(
      (conversation) => {
        dispatch({ type: "renamed", conversation });
        return true;
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
        return false;
      }
    );

  /** Resolves `false` when the delete failed. */
  const remove = (id: string): Promise<boolean> =>
    conversationDelete(id).then(
      () => {
        dispatch({ type: "deleted", id });
        return true;
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
        return false;
      }
    );

  /** Resolves `false` when nothing was copied; the error says why. */
  const copyMarkdown = (id: string): Promise<boolean> =>
    copyConversationMarkdown(id).then((error) => {
      if (error !== null) {
        dispatch({ type: "failed", error });
      }
      return error === null;
    });

  return { state, select, created, touched, rename, remove, copyMarkdown };
};

export type { ConversationsState };
export {
  conversationsReducer,
  copyConversationMarkdown,
  initialConversations,
  useConversations,
};
