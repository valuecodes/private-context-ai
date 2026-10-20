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
    | "ollama"
    | "wrongKey"
    | "notFound"
    | "storageUnavailable"
    | "database";
  message: string;
};

/** What the `storage_status` command returns. */
type StorageStatus =
  | { state: "ok" }
  | { state: "keychainUnavailable"; detail: string }
  | { state: "keyMissing"; dbPath: string }
  | { state: "wrongKey"; dbPath: string }
  | { state: "io"; detail: string }
  | { state: "alreadyRunning" };

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

/** A saved conversation, without its messages. */
type Conversation = {
  id: string;
  title: string;
  /** Empty until the first send when no model was picked. */
  model: string;
  createdAt: string;
  updatedAt: string;
};

/** `pending`: still streaming, or the app stopped while it was. */
type MessageStatus = "pending" | "complete" | "cancelled" | "error";

type StoredMessage = {
  id: string;
  role: ChatRole;
  content: string;
  status: MessageStatus;
  createdAt: string;
  promptTokens: number | null;
  completionTokens: number | null;
  durationMs: number | null;
};

/** What the `conversation_get` command returns. */
type ConversationWithMessages = {
  conversation: Conversation;
  messages: StoredMessage[];
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
  Conversation,
  ConversationWithMessages,
  ModelInfo,
  OllamaStatus,
  RequestId,
  StorageStatus,
  StoredMessage,
};
