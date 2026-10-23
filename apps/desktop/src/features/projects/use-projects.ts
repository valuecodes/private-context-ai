import { useEffect, useReducer, useRef } from "react";

import { toAppError } from "~/lib/app-error";
import {
  projectCreate,
  projectDelete,
  projectList,
  projectRename,
  projectUpdateInstructions,
} from "~/lib/ipc";
import type { AppError, Project } from "~/lib/types";

type ProjectsState = {
  phase: "loading" | "ready";
  /** Sorted by name. */
  projects: Project[];
  error: AppError | null;
};

type ProjectsAction =
  | { type: "loaded"; projects: Project[] }
  /** Created, renamed or given new instructions. */
  | { type: "saved"; project: Project }
  | { type: "deleted"; id: string }
  | { type: "failed"; error: AppError };

const initialProjects: ProjectsState = {
  phase: "loading",
  projects: [],
  error: null,
};

const byName = (projects: Project[]): Project[] =>
  projects.toSorted(
    (a, b) =>
      a.name.localeCompare(b.name) || a.createdAt.localeCompare(b.createdAt)
  );

const projectsReducer = (
  state: ProjectsState,
  action: ProjectsAction
): ProjectsState => {
  if (action.type === "loaded") {
    // Keep projects created while the list was loading.
    const created = state.projects.filter(
      (local) => !action.projects.some(({ id }) => id === local.id)
    );
    return {
      phase: "ready",
      projects: byName([...created, ...action.projects]),
      error: null,
    };
  }
  if (action.type === "saved") {
    const others = state.projects.filter(({ id }) => id !== action.project.id);
    return {
      ...state,
      projects: byName([...others, action.project]),
      error: null,
    };
  }
  if (action.type === "deleted") {
    return {
      ...state,
      projects: state.projects.filter(({ id }) => id !== action.id),
      error: null,
    };
  }
  return { ...state, phase: "ready", error: action.error };
};

/** The projects, and saving their names and instructions. */
const useProjects = () => {
  const [state, dispatch] = useReducer(projectsReducer, initialProjects);
  // Instruction saves run one at a time, in order; `true` while every save
  // since the last `settled` has succeeded.
  const saves = useRef<Promise<boolean>>(Promise.resolve(true));

  useEffect(() => {
    let cancelled = false;
    void projectList().then(
      (projects) => {
        if (!cancelled) {
          dispatch({ type: "loaded", projects });
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

  /** Resolves with the new project, or `null` when Rust refused it. */
  const create = (name: string): Promise<Project | null> =>
    projectCreate(name).then(
      (project) => {
        dispatch({ type: "saved", project });
        return project;
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
        return null;
      }
    );

  /** Resolves `false` when Rust refused the name. */
  const rename = (id: string, name: string): Promise<boolean> =>
    projectRename(id, name).then(
      (project) => {
        dispatch({ type: "saved", project });
        return true;
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
        return false;
      }
    );

  /** Queues a save of the project's instructions; a failure shows its error. */
  const saveInstructions = (id: string, instructions: string) => {
    saves.current = saves.current.then((ok) =>
      projectUpdateInstructions(id, instructions).then(
        (project) => {
          dispatch({ type: "saved", project });
          return ok;
        },
        (error: unknown) => {
          dispatch({ type: "failed", error: toAppError(error) });
          return false;
        }
      )
    );
  };

  /**
   * Resolves once every queued instructions save has finished: `false` when
   * one of them failed since the last call.
   */
  const settled = (): Promise<boolean> => {
    const result = saves.current;
    saves.current = result.then(() => true);
    return result;
  };

  /** Resolves `false` when the delete failed. */
  const remove = (id: string): Promise<boolean> =>
    projectDelete(id).then(
      () => {
        dispatch({ type: "deleted", id });
        return true;
      },
      (error: unknown) => {
        dispatch({ type: "failed", error: toAppError(error) });
        return false;
      }
    );

  return { state, create, rename, saveInstructions, settled, remove };
};

export type { ProjectsState };
export { initialProjects, projectsReducer, useProjects };
