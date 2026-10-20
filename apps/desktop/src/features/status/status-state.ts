import { toAppError } from "~/lib/app-error";
import { ollamaStatus } from "~/lib/ipc";
import type { AppError, OllamaStatus } from "~/lib/types";

type StatusState =
  | { phase: "checking" }
  | { phase: "ready"; status: OllamaStatus }
  | { phase: "failed"; error: AppError };

const loadOllamaStatus = async (): Promise<StatusState> => {
  try {
    return { phase: "ready", status: await ollamaStatus() };
  } catch (error) {
    return { phase: "failed", error: toAppError(error) };
  }
};

const isReachable = (state: StatusState): boolean =>
  state.phase === "ready" && state.status.reachable;

/** The status line text, and whether a retry can help. */
const describeStatus = (
  state: StatusState
): { text: string; canRetry: boolean } => {
  if (state.phase === "checking") {
    return { text: "Ollama: checking…", canRetry: false };
  }
  if (state.phase === "ready") {
    const { reachable, version, baseUrl } = state.status;
    if (!reachable) {
      return {
        text: `Ollama: not running at ${baseUrl}. Start Ollama, then retry.`,
        canRetry: true,
      };
    }
    const versionText = version === null ? "" : ` v${version}`;
    return {
      text: `Ollama: running${versionText} at ${baseUrl}`,
      canRetry: false,
    };
  }
  // A rejected URL is fixed by configuration, not by retrying.
  if (state.error.kind === "invalidBaseUrl") {
    return {
      text: `Ollama URL rejected: ${state.error.message}`,
      canRetry: false,
    };
  }
  return { text: `Ollama: ${state.error.message}`, canRetry: true };
};

export type { StatusState };
export { describeStatus, isReachable, loadOllamaStatus };
