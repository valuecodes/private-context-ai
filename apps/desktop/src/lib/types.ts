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

/**
 * What a chat saves as its Think setting. Rust decides from the model what is
 * actually sent; off sends `false`, or the default level to a model that
 * always thinks.
 */
type ThinkChoice =
  | { kind: "off" }
  | { kind: "on" }
  | { kind: "level"; level: ThinkLevel };

/** Sampling options a chat saves. Field names are Ollama's. */
type ChatOptions = {
  /** 0 to 2; unset is the model's default. */
  temperature?: number;
  /** Context window in tokens; unset is the model's default. */
  num_ctx?: number;
};

/** How a chat is sent, applied from its next send. */
type ConversationSettings = {
  /** Sent first as a system message when not blank; never shown as one. */
  systemPrompt: string;
  options: ChatOptions;
  think: ThinkChoice;
};

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
  settings: ConversationSettings;
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

/** The id a send command returns, for `chat_cancel`. */
type RequestId = string;

/** What every send command returns once Rust admitted it. */
type SendStarted = {
  requestId: RequestId;
  /** The prompt being answered: new, or the existing one when regenerating. */
  userMessageId: string;
  assistantMessageId: string;
};

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
  ChatOptions,
  Conversation,
  ConversationSettings,
  ConversationWithMessages,
  LogLevel,
  ModelCapabilities,
  ModelInfo,
  OllamaStatus,
  RequestId,
  SendStarted,
  Settings,
  SettingsPatch,
  StorageStatus,
  StoredMessage,
  ThinkChoice,
  ThinkLevel,
  ThinkStyle,
};
