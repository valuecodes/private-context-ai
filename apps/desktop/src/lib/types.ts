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
  /**
   * Offers the display-only `render_chart` tool to models that list `tools`.
   * Off unless the user turns it on.
   */
  charts: boolean;
};

/** A chart the model asked for, validated by Rust. Its text is the model's. */
type ChartSpec = {
  title: string;
  type: "bar" | "line" | "pie";
  /** As many as `values`, 1 to 100. */
  labels: string[];
  /** Finite; a pie's are not negative. */
  values: number[];
};

/** One `render_chart` call of a reply: drawn, or a note saying why not. */
type ChartEntry = {
  /** Where it goes in the reply text, in UTF-16 code units. */
  offset: number;
  spec: ChartSpec | null;
  /** Why it was not shown; fixed text from Rust. */
  error?: string;
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
  /** The project it belongs to; `null` outside any project. */
  projectId: string | null;
};

/** A project: its instructions are sent ahead of every chat's own prompt. */
type Project = {
  id: string;
  name: string;
  instructions: string;
  createdAt: string;
  updatedAt: string;
  /** Whether its chats' memory is written without asking; on by default. */
  autoMemory: boolean;
};

/**
 * `pending`: newer messages are not in it yet. `stale`: the chat was edited,
 * so it is not sent until it is rebuilt.
 */
type MemoryState = "current" | "pending" | "stale";

/** One chat's model-written summary, sent to the project's other chats. */
type MemoryEntry = {
  conversationId: string;
  /** The chat's title. */
  title: string;
  content: string;
  model: string | null;
  updatedAt: string;
  userEdited: boolean;
  /** The last refresh rebuilt it after the chat was edited. */
  rebuilt: boolean;
  /** A message too long for one summary request was shortened. */
  truncated: boolean;
  state: MemoryState;
};

/** How one chat of `project_memory_refresh` went. */
type RefreshResult = {
  conversationId: string;
  outcome: "updated" | "current" | "cancelled" | "busy" | "failed";
  error: AppError | null;
};

/** What `project_context_preview` returns: nothing is sent to get it. */
type ContextPreview = {
  /** The exact `system` message; empty when there is none. */
  text: string;
  /** Newest first: in the message, over the budget, or out of date. */
  entries: {
    conversationId: string;
    title: string;
    state: "included" | "dropped" | "stale";
  }[];
  memoryBytes: number;
  budgetBytes: number;
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
  /** Generating the completion tokens only; null for older replies. */
  evalMs: number | null;
  /** The model that wrote a reply; null for prompts and older replies. */
  model: string | null;
  /** A compared prompt and its candidate replies, until one is picked. */
  compareGroup: string | null;
  /** A reply's `render_chart` calls; empty for most. */
  charts: ChartEntry[];
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
  | { type: "chart"; offset: number; spec: ChartSpec }
  | { type: "chartError"; offset: number; reason: string }
  | {
      type: "done";
      promptTokens: number | null;
      completionTokens: number | null;
      durationMs: number | null;
      doneReason: string | null;
      /** From the first thinking chunk to the first answer chunk. */
      thinkingMs: number | null;
      /** Generating the completion tokens only, for tokens/s. */
      evalMs: number | null;
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

/** What `chat_compare` returns once Rust admitted it. */
type CompareStarted = {
  /** Cancels the whole comparison: the running model and the rest. */
  requestId: RequestId;
  userMessageId: string;
  compareGroup: string;
  /** In run order. */
  candidates: { messageId: string; model: string }[];
};

/** Streamed by `chat_compare` over its channel. */
type CompareEvent =
  | { type: "candidate"; messageId: string; event: ChatEvent }
  /** Every candidate has ended. */
  | { type: "finished" };

/** `attempt`: started, no outcome yet (or the app stopped while it ran). */
type AuditStatus = "attempt" | "ok" | "error" | "cancelled";

/** One row of `activity_list`. Metadata only, never message content. */
type AuditEvent = {
  id: number;
  /** ISO-8601 UTC. */
  at: string;
  /**
   * `ollama.chat`, `ollama.memory`, `ollama.show`, `ollama.tags`,
   * `ollama.version`, `app.start`, `db.open`.
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
  ChartEntry,
  ChartSpec,
  ChatEvent,
  ChatOptions,
  CompareEvent,
  CompareStarted,
  ContextPreview,
  Conversation,
  ConversationSettings,
  ConversationWithMessages,
  LogLevel,
  MemoryEntry,
  ModelCapabilities,
  ModelInfo,
  OllamaStatus,
  Project,
  RefreshResult,
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
