/** What the `app_info` command returns. */
type AppInfo = {
  version: string;
};

/** Mirror of the Rust `AppError`: every rejected command carries one. */
type AppError = {
  kind:
    | "io"
    | "internal"
    | "invalidBaseUrl"
    | "invalidInput"
    | "busy"
    | "tooLarge"
    | "timeout"
    | "unreachable"
    | "ollama";
  message: string;
};

/** What the `ollama_status` command returns. */
type OllamaStatus = {
  reachable: boolean;
  version: string | null;
  baseUrl: string;
};

/** One entry of the `ollama_list_models` result. */
type ModelInfo = {
  name: string;
  sizeBytes: number | null;
  parameterSize: string | null;
};

type ChatRole = "system" | "user" | "assistant";

/** One turn as `chat_send` receives it. */
type ChatMessage = {
  role: ChatRole;
  content: string;
};

/** Streamed by `chat_send` over its channel. */
type ChatEvent =
  | { type: "token"; content: string }
  | {
      type: "done";
      promptTokens: number | null;
      completionTokens: number | null;
      durationMs: number | null;
      doneReason: string | null;
    }
  | { type: "error"; error: AppError };

/** The id `chat_send` returns, for `chat_cancel`. */
type RequestId = string;

export type {
  AppError,
  AppInfo,
  ChatEvent,
  ChatMessage,
  ModelInfo,
  OllamaStatus,
  RequestId,
};
