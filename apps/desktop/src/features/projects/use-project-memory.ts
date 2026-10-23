import { useCallback, useEffect, useReducer } from "react";

import { toAppError } from "~/lib/app-error";
import {
  projectMemoryDelete,
  projectMemoryList,
  projectMemoryRefresh,
  projectMemoryUpdate,
} from "~/lib/ipc";
import type {
  AppError,
  MemoryEntry,
  MemoryList,
  RefreshResult,
  UnsavedChat,
} from "~/lib/types";

type MemoryState = {
  phase: "loading" | "ready";
  /** Newest first, as Rust lists them. */
  entries: MemoryEntry[];
  /** Chats with replies and no entry yet, oldest first. */
  unsaved: UnsavedChat[];
  /** The chat being refreshed, `all` for the whole project, or `null`. */
  refreshing: string | null;
  /** The chats the last refresh could not summarize, with why. */
  failures: RefreshResult[];
  error: AppError | null;
};

type MemoryAction =
  | { type: "loaded"; list: MemoryList }
  | { type: "saved"; entry: MemoryEntry }
  | { type: "deleted"; conversationId: string }
  | { type: "refreshing"; target: string }
  | { type: "refreshed"; results: RefreshResult[] }
  | { type: "failed"; error: AppError };

const initialMemory: MemoryState = {
  phase: "loading",
  entries: [],
  unsaved: [],
  refreshing: null,
  failures: [],
  error: null,
};

const memoryReducer = (
  state: MemoryState,
  action: MemoryAction
): MemoryState => {
  if (action.type === "loaded") {
    return {
      ...state,
      phase: "ready",
      entries: action.list.entries,
      unsaved: action.list.unsaved,
    };
  }
  if (action.type === "saved") {
    // An edit makes it the newest entry, as Rust would list it.
    const others = state.entries.filter(
      ({ conversationId }) => conversationId !== action.entry.conversationId
    );
    return { ...state, entries: [action.entry, ...others], error: null };
  }
  if (action.type === "deleted") {
    return {
      ...state,
      entries: state.entries.filter(
        ({ conversationId }) => conversationId !== action.conversationId
      ),
      error: null,
    };
  }
  if (action.type === "refreshing") {
    return { ...state, refreshing: action.target, failures: [], error: null };
  }
  if (action.type === "refreshed") {
    return {
      ...state,
      refreshing: null,
      failures: action.results.filter(
        ({ outcome }) => outcome === "failed" || outcome === "busy"
      ),
    };
  }
  return { ...state, phase: "ready", refreshing: null, error: action.error };
};

/**
 * The project's memory entries, and editing, deleting and refreshing them.
 * `onChanged` runs after each change, so pending counts elsewhere follow.
 */
const useProjectMemory = (projectId: string, onChanged?: () => void) => {
  const [state, dispatch] = useReducer(memoryReducer, initialMemory);

  const reload = useCallback(
    (): Promise<void> =>
      projectMemoryList(projectId).then(
        (list) => {
          dispatch({ type: "loaded", list });
        },
        (error: unknown) => {
          dispatch({ type: "failed", error: toAppError(error) });
        }
      ),
    [projectId]
  );

  useEffect(() => {
    void reload();
  }, [reload]);

  /** Resolves `false` when Rust refused the text, keeping the editor open. */
  const update = (conversationId: string, content: string): Promise<boolean> =>
    projectMemoryUpdate(conversationId, content).then(
      (entry) => {
        dispatch({ type: "saved", entry });
        onChanged?.();
        return true;
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
        return false;
      }
    );

  const remove = (conversationId: string) => {
    void projectMemoryDelete(conversationId).then(
      () => {
        dispatch({ type: "deleted", conversationId });
        onChanged?.();
        // The chat may now be listed as not saved.
        void reload();
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
      }
    );
  };

  /** One chat, or every chat of the project with `null`; then reloads. */
  const refresh = (conversationId: string | null) => {
    dispatch({ type: "refreshing", target: conversationId ?? "all" });
    void projectMemoryRefresh(projectId, conversationId)
      .then(
        (results) => {
          dispatch({ type: "refreshed", results });
        },
        (error: unknown) => {
          dispatch({ type: "failed", error: toAppError(error) });
        }
      )
      .then(reload)
      .then(onChanged);
  };

  return { state, reload, update, remove, refresh };
};

export type { MemoryState };
export { initialMemory, memoryReducer, useProjectMemory };
