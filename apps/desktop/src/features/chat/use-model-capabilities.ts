import { useCallback, useEffect, useRef, useState } from "react";

import { ollamaModelCapabilities } from "~/lib/ipc";
import type { ModelCapabilities } from "~/lib/types";

/** Capabilities known so far, and the lookups running or failed. */
type CapabilitiesState = {
  /** Bumped by `reset`; a result from an older generation is dropped. */
  generation: number;
  known: Record<string, ModelCapabilities>;
  pending: string[];
  failed: string[];
};

type CapabilitiesAction =
  | { type: "reset" }
  | { type: "started"; generation: number; names: string[] }
  | {
      type: "loaded";
      generation: number;
      name: string;
      caps: ModelCapabilities;
    }
  | { type: "failed"; generation: number; name: string };

const INITIAL: CapabilitiesState = {
  generation: 0,
  known: {},
  pending: [],
  failed: [],
};

const capabilitiesReducer = (
  state: CapabilitiesState,
  action: CapabilitiesAction
): CapabilitiesState => {
  if (action.type === "reset") {
    return { ...INITIAL, generation: state.generation + 1 };
  }
  if (action.generation !== state.generation) {
    return state;
  }
  if (action.type === "started") {
    const names = action.names.filter(
      (name) => !(name in state.known) && !state.pending.includes(name)
    );
    return names.length === 0
      ? state
      : {
          ...state,
          pending: [...state.pending, ...names],
          failed: state.failed.filter((name) => !names.includes(name)),
        };
  }
  if (action.type === "loaded") {
    return {
      ...state,
      known: { ...state.known, [action.name]: action.caps },
      pending: state.pending.filter((name) => name !== action.name),
    };
  }
  return {
    ...state,
    pending: state.pending.filter((name) => name !== action.name),
    failed: [...state.failed, action.name],
  };
};

/**
 * The names to look up: not known, not already running, and not failed
 * before unless `retryFailed`.
 */
const namesToStart = (
  state: CapabilitiesState,
  names: string[],
  { retryFailed }: { retryFailed: boolean }
): string[] =>
  [...new Set(names)].filter(
    (name) =>
      !(name in state.known) &&
      !state.pending.includes(name) &&
      (retryFailed || !state.failed.includes(name))
  );

type ModelCapabilitiesOptions = {
  reachable: boolean;
  /** Changes when the model list reloads: another Ollama, other models. */
  listVersion: number;
};

/**
 * Model capabilities, one audited `/api/show` per model: a lookup already
 * running is never started again, and results from before a list reload are
 * dropped. Call it before any effect that uses `lookUp`, so a reload's reset
 * runs first.
 */
const useModelCapabilities = ({
  reachable,
  listVersion,
}: ModelCapabilitiesOptions) => {
  const [state, setState] = useState(INITIAL);
  // Read synchronously, so two requests in one tick see each other.
  const current = useRef(INITIAL);

  const apply = useCallback((action: CapabilitiesAction) => {
    const next = capabilitiesReducer(current.current, action);
    if (next !== current.current) {
      current.current = next;
      setState(next);
    }
  }, []);

  const request = useCallback(
    (names: string[], retryFailed: boolean) => {
      const start = namesToStart(current.current, names, { retryFailed });
      if (start.length === 0) {
        return;
      }
      const { generation } = current.current;
      apply({ type: "started", generation, names: start });
      for (const name of start) {
        void ollamaModelCapabilities(name).then(
          (caps) => {
            apply({ type: "loaded", generation, name, caps });
          },
          () => {
            apply({ type: "failed", generation, name });
          }
        );
      }
    },
    [apply]
  );

  useEffect(() => {
    apply({ type: "reset" });
  }, [listVersion, apply]);

  /** Looks up one model, such as the selected one; never retries a failure. */
  const lookUp = useCallback(
    (name: string) => {
      if (reachable && name !== "") {
        request([name], false);
      }
    },
    [reachable, request]
  );

  /** Looks up every model not known yet, retrying earlier failures. */
  const loadAll = useCallback(
    (names: string[]) => {
      if (reachable) {
        request(names, true);
      }
    },
    [reachable, request]
  );

  return { capabilities: state.known, lookUp, loadAll };
};

export type { CapabilitiesState };
export {
  capabilitiesReducer,
  INITIAL as INITIAL_CAPABILITIES,
  namesToStart,
  useModelCapabilities,
};
