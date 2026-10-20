import type { AppError } from "~/lib/types";

const APP_ERROR_KINDS = new Set<string>([
  "io",
  "internal",
  "invalidBaseUrl",
  "invalidInput",
  "busy",
  "tooLarge",
  "timeout",
  "unreachable",
  "ollama",
  "wrongKey",
  "notFound",
  "storageUnavailable",
  "database",
] satisfies AppError["kind"][]);

const isAppError = (value: unknown): value is AppError =>
  typeof value === "object" &&
  value !== null &&
  "kind" in value &&
  "message" in value &&
  typeof value.kind === "string" &&
  APP_ERROR_KINDS.has(value.kind) &&
  typeof value.message === "string";

/**
 * A rejected invoke as an `AppError`. Tauri rejects with a plain string when
 * a call never reached the command (bad arguments, unknown command); that and
 * anything else unexpected becomes `internal`.
 */
const toAppError = (value: unknown): AppError => {
  if (isAppError(value)) {
    return value;
  }
  return {
    kind: "internal",
    message: typeof value === "string" ? value : "unexpected error",
  };
};

export { toAppError };
