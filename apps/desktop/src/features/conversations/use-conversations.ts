import { useEffect, useReducer, useRef } from "react";

import { toAppError } from "~/lib/app-error";
import { copyLater, copyText } from "~/lib/clipboard";
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
  /** Rust deleted the project and, with it, its chats. */
  | { type: "projectDeleted"; projectId: string }
  /** `at`: when the send started, as an ISO string like Rust's. */
  | { type: "touched"; id: string; model: string; at: string }
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
  if (action.type === "projectDeleted") {
    const gone = (conversation: Conversation) =>
      conversation.projectId === action.projectId;
    const selected = state.conversations.find(
      ({ id }) => id === state.selectedId
    );
    return {
      ...state,
      conversations: state.conversations.filter(
        (conversation) => !gone(conversation)
      ),
      selectedId:
        selected !== undefined && gone(selected) ? null : state.selectedId,
      error: null,
    };
  }
  if (action.type === "touched") {
    // Rust saved the model and the time with the send; keep the cached copy
    // in step, so the chat moves under Today without a reload.
    const updated = state.conversations.map((conversation) =>
      conversation.id === action.id
        ? { ...conversation, model: action.model, updatedAt: action.at }
        : conversation
    );
    return { ...state, conversations: moveToTop(updated, action.id) };
  }
  return { ...state, phase: "ready", error: action.error };
};

/**
 * Copies a conversation as Markdown; resolves `null` once copied, else why
 * not. Call it straight from the click. With `prepared` (fetched on hover)
 * the copy is synchronous, which the textarea fallback needs: WebKitGTK has
 * no `ClipboardItem`, and a copy after an IPC `await` has lost the click.
 * Without it, the clipboard write starts before Rust answers.
 */
const copyConversationMarkdown = (
  id: string,
  prepared?: string
): Promise<AppError | null> => {
  if (prepared !== undefined) {
    return copyText(prepared).then((copied) => (copied ? null : COPY_FAILED));
  }
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
  /**
   * Markdown fetched on hover, by conversation id. Each fetch fills its own
   * entry, so one that lands after a copy took the entry fills nothing.
   */
  const prepared = useRef(new Map<string, { markdown?: string }>());

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
    dispatch({ type: "touched", id, model, at: new Date().toISOString() });
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

  /** Drops the chats of a project Rust has deleted. */
  const projectDeleted = (projectId: string) => {
    dispatch({ type: "projectDeleted", projectId });
  };

  /**
   * Fetches a conversation's Markdown ahead of a click on its copy button,
   * so the copy can run inside the click. A failed fetch is left to the
   * click, which fetches again and reports it.
   */
  const prepareMarkdown = (id: string) => {
    const entry: { markdown?: string } = {};
    prepared.current.set(id, entry);
    void conversationMarkdown(id).then(
      (markdown) => {
        entry.markdown = markdown;
      },
      () => undefined
    );
  };

  /** Resolves `false` when nothing was copied; the error says why. */
  const copyMarkdown = (id: string): Promise<boolean> => {
    // Used once, so the next hover fetches the conversation as it is then.
    const markdown = prepared.current.get(id)?.markdown;
    prepared.current.delete(id);
    return copyConversationMarkdown(id, markdown).then((error) => {
      if (error !== null) {
        dispatch({ type: "failed", error });
      }
      return error === null;
    });
  };

  return {
    state,
    select,
    created,
    touched,
    rename,
    remove,
    projectDeleted,
    prepareMarkdown,
    copyMarkdown,
  };
};

export type { ConversationsState };
export {
  conversationsReducer,
  copyConversationMarkdown,
  initialConversations,
  useConversations,
};
