import { useCallback, useEffect, useReducer } from "react";

import { toAppError } from "~/lib/app-error";
import {
  projectFileAdd,
  projectFileAddPdf,
  projectFileDelete,
  projectFileList,
} from "~/lib/ipc";
import type { AppError, FilePage, ProjectFile } from "~/lib/types";

import type { DropEntry, PickedFile, PreparedFile } from "./read-files";
import { collectDropped, prepareFile } from "./read-files";

/** Files loaded per page. */
const PAGE_SIZE = 200;

type SkippedFile = { name: string; reason: string };

type FilesState = {
  phase: "loading" | "ready";
  /** The first pages of the project's files, by name, as Rust lists them. */
  files: ProjectFile[];
  /** How many files the project has in all. */
  total: number;
  /** While files are being added: how many are done, of how many. */
  adding: { done: number; of: number } | null;
  /** The file being deleted. */
  deleting: string | null;
  /** The files the last add left out, with why. */
  skipped: SkippedFile[];
  error: AppError | null;
};

type FilesAction =
  | { type: "loaded"; page: FilePage; offset: number }
  | { type: "adding"; of: number }
  | { type: "progress"; done: number }
  | { type: "skipped"; file: SkippedFile }
  | { type: "added" }
  | { type: "deleting"; id: string }
  | { type: "deleted"; id: string }
  | { type: "failed"; error: AppError };

const initialFiles: FilesState = {
  phase: "loading",
  files: [],
  total: 0,
  adding: null,
  deleting: null,
  skipped: [],
  error: null,
};

/** An add or a delete is running: the controls wait for it. */
const isBusy = (state: FilesState): boolean =>
  state.adding !== null || state.deleting !== null;

const filesReducer = (state: FilesState, action: FilesAction): FilesState => {
  if (action.type === "loaded") {
    // A later page counts only if it starts where the list ends: a second
    // "Show more" for the same offset, or one from before a reload, is dropped.
    if (action.offset > 0 && action.offset !== state.files.length) {
      return state;
    }
    return {
      ...state,
      phase: "ready",
      files:
        action.offset === 0
          ? action.page.files
          : [...state.files, ...action.page.files],
      total: action.page.total,
    };
  }
  if (action.type === "adding") {
    return {
      ...state,
      adding: { done: 0, of: action.of },
      skipped: [],
      error: null,
    };
  }
  if (action.type === "progress") {
    return state.adding === null
      ? state
      : { ...state, adding: { ...state.adding, done: action.done } };
  }
  if (action.type === "skipped") {
    return { ...state, skipped: [...state.skipped, action.file] };
  }
  if (action.type === "added") {
    return { ...state, adding: null };
  }
  if (action.type === "deleting") {
    return { ...state, deleting: action.id, error: null };
  }
  if (action.type === "deleted") {
    // Rust's list loses the same row, so the next page's offset still holds.
    const files = state.files.filter(({ id }) => id !== action.id);
    return {
      ...state,
      files,
      total: state.total - (state.files.length - files.length),
      deleting: null,
    };
  }
  return { ...state, phase: "ready", deleting: null, error: action.error };
};

/**
 * Sends a prepared file to Rust: text as it is, a PDF for its text. Resolves
 * to why it was skipped, or `null` once it is stored.
 */
const storeFile = async (
  projectId: string,
  prepared: PreparedFile
): Promise<SkippedFile | null> => {
  if (prepared.kind === "skipped") {
    return prepared;
  }
  const { name } = prepared;
  try {
    await (prepared.kind === "pdf"
      ? projectFileAddPdf(projectId, name, prepared.bytes)
      : projectFileAdd(projectId, name, prepared.content));
    return null;
  } catch (error) {
    return { name, reason: toAppError(error).message };
  }
};

/**
 * One project's files: loading them page by page, adding and deleting.
 * Files are added one at a time, in order; afterwards the list reloads from
 * the first page, since new names can land anywhere in it.
 */
const useProjectFiles = (projectId: string) => {
  const [state, dispatch] = useReducer(filesReducer, initialFiles);

  const load = useCallback(
    (offset: number): Promise<void> =>
      projectFileList(projectId, offset, PAGE_SIZE).then(
        (page) => {
          dispatch({ type: "loaded", page, offset });
        },
        (error: unknown) => {
          dispatch({ type: "failed", error: toAppError(error) });
        }
      ),
    [projectId]
  );

  useEffect(() => {
    void load(0);
  }, [load]);

  const more = () => {
    void load(state.files.length);
  };

  const add = async (picked: PickedFile[]): Promise<void> => {
    if (picked.length === 0) {
      dispatch({ type: "added" });
      return;
    }
    dispatch({ type: "adding", of: picked.length });
    for (const [index, file] of picked.entries()) {
      const skipped = await storeFile(projectId, await prepareFile(file));
      if (skipped !== null) {
        dispatch({ type: "skipped", file: skipped });
      }
      dispatch({ type: "progress", done: index + 1 });
    }
    dispatch({ type: "added" });
    await load(0);
  };

  /**
   * A drop's folders are walked first, then added like picked files. The
   * panel is busy from the start, with nothing to count yet.
   */
  const addDropped = (entries: DropEntry[]) => {
    dispatch({ type: "adding", of: 0 });
    void collectDropped(entries).then(add, (error: unknown) => {
      dispatch({ type: "added" });
      dispatch({ type: "failed", error: toAppError(error) });
    });
  };

  const remove = (id: string) => {
    dispatch({ type: "deleting", id });
    void projectFileDelete(projectId, id).then(
      () => {
        dispatch({ type: "deleted", id });
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
      }
    );
  };

  return {
    state,
    more,
    add: (picked: PickedFile[]) => {
      void add(picked);
    },
    addDropped,
    remove,
  };
};

export type { FilesState };
export { filesReducer, initialFiles, isBusy, storeFile, useProjectFiles };
