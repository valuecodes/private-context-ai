/** What the `app_info` command returns. */
type AppInfo = {
  version: string;
  /** Where the log files are, shown as text. */
  logDir: string;
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

/** A model-defined level (`low`, `high`, `max`, …), as `/api/show` lists it. */
type ThinkLevel = string;

/** Ollama's `think`: on/off, or a level for models that take one. */
type Think = boolean | ThinkLevel;

/** How a thinking model is switched. */
type ThinkStyle =
  | { kind: "bool" }
  | {
      kind: "levels";
      levels: ThinkLevel[];
      default: ThinkLevel;
      /** `false` turns thinking off; models that always think refuse it. */
      canDisable: boolean;
    };

/** What `ollama_model_capabilities` returns, from an audited `/api/show`. */
type ModelCapabilities = {
  thinking: boolean;
  vision: boolean;
  tools: boolean;
  embeddingOnly: boolean;
  thinkStyle: ThinkStyle;
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
  /** The model's reasoning; never sent back to it. */
  thinking: string | null;
  thinkingMs: number | null;
};

/** What the `conversation_get` command returns. */
type ConversationWithMessages = {
  conversation: Conversation;
  messages: StoredMessage[];
};

/** Streamed by `chat_send` over its channel. */
type ChatEvent =
  | { type: "token"; content: string }
  | { type: "thinking"; content: string }
  | {
      type: "done";
      promptTokens: number | null;
      completionTokens: number | null;
      durationMs: number | null;
      doneReason: string | null;
      /** From the first thinking chunk to the first answer chunk. */
      thinkingMs: number | null;
    }
  | { type: "error"; error: AppError };

/** The id `chat_send` returns, for `chat_cancel`. */
type RequestId = string;

/** `attempt`: started, no outcome yet (or the app stopped while it ran). */
type AuditStatus = "attempt" | "ok" | "error" | "cancelled";

/** One row of `activity_list`. Metadata only, never message content. */
type AuditEvent = {
  id: number;
  /** ISO-8601 UTC. */
  at: string;
  /**
   * `ollama.chat`, `ollama.show`, `ollama.tags`, `ollama.version`,
   * `app.start`, `db.open`.
   */
  kind: string;
  /** `host:port`; null for lifecycle events. */
  host: string | null;
  endpoint: string | null;
  model: string | null;
  status: AuditStatus;
  promptTokens: number | null;
  completionTokens: number | null;
  durationMs: number | null;
  detail: string | null;
};

type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

/** What `settings_get` and `settings_set` return. */
type Settings = {
  ollamaBaseUrl: string;
  /** Empty when none was picked. */
  defaultModel: string;
  logLevel: LogLevel;
};

/** What `settings_set` takes: only the fields to change. */
type SettingsPatch = Partial<Settings>;

export type {
  AppError,
  AppInfo,
  AuditEvent,
  AuditStatus,
  ChatEvent,
  Conversation,
  ConversationWithMessages,
  LogLevel,
  ModelCapabilities,
  ModelInfo,
  OllamaStatus,
  RequestId,
  Settings,
  SettingsPatch,
  StorageStatus,
  StoredMessage,
  Think,
  ThinkLevel,
  ThinkStyle,
};
