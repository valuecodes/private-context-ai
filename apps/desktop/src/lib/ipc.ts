// The only module that imports `@tauri-apps/api`. Everything else calls these
// typed wrappers, so `vi.mock("~/lib/ipc")` is the single test seam.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type {
  AppInfo,
  AuditEvent,
  ChatEvent,
  CompareEvent,
  CompareStarted,
  ContextPreview,
  Conversation,
  ConversationSettings,
  ConversationWithMessages,
  FilePage,
  MemoryEntry,
  MemoryList,
  MemoryStatus,
  ModelCapabilities,
  ModelInfo,
  ModelStats,
  OllamaStatus,
  PendingCount,
  Project,
  ProjectFile,
  RefreshResult,
  RequestId,
  SendStarted,
  Settings,
  SettingsPatch,
  StorageStatus,
} from "~/lib/types";

const appInfo = (): Promise<AppInfo> => invoke<AppInfo>("app_info");

const storageStatus = (): Promise<StorageStatus> =>
  invoke<StorageStatus>("storage_status");

const ollamaStatus = (): Promise<OllamaStatus> =>
  invoke<OllamaStatus>("ollama_status");

const ollamaListModels = (): Promise<ModelInfo[]> =>
  invoke<ModelInfo[]>("ollama_list_models");

/** Audited once per model; Rust caches the answer. */
const ollamaModelCapabilities = (model: string): Promise<ModelCapabilities> =>
  invoke<ModelCapabilities>("ollama_model_capabilities", { model });

/**
 * Saved with `settings` at once, so its first send already uses them; in
 * project `projectId` unless it is `null`.
 */
const conversationCreate = (
  title: string | null,
  model: string,
  settings: ConversationSettings,
  projectId: string | null
): Promise<Conversation> =>
  invoke<Conversation>("conversation_create", {
    title,
    model,
    settings,
    projectId,
  });

const conversationList = (): Promise<Conversation[]> =>
  invoke<Conversation[]>("conversation_list");

const conversationGet = (id: string): Promise<ConversationWithMessages> =>
  invoke<ConversationWithMessages>("conversation_get", { id });

/** Ids of conversations whose messages contain `query`, newest first. */
const conversationSearch = (query: string): Promise<string[]> =>
  invoke<string[]>("conversation_search", { query });

/** The conversation as a Markdown document; nothing is written to disk. */
const conversationMarkdown = (id: string): Promise<string> =>
  invoke<string>("conversation_markdown", { id });

const conversationRename = (id: string, title: string): Promise<Conversation> =>
  invoke<Conversation>("conversation_rename", { id, title });

/** Applies from the conversation's next send. */
const conversationUpdateSettings = (
  id: string,
  settings: ConversationSettings
): Promise<Conversation> =>
  invoke<Conversation>("conversation_update_settings", { id, settings });

const conversationDelete = (id: string): Promise<void> =>
  invoke<undefined>("conversation_delete", { id });

const projectCreate = (name: string): Promise<Project> =>
  invoke<Project>("project_create", { name });

const projectList = (): Promise<Project[]> => invoke<Project[]>("project_list");

const projectRename = (id: string, name: string): Promise<Project> =>
  invoke<Project>("project_rename", { id, name });

/** Applies from the next send of every chat in the project. */
const projectUpdateInstructions = (
  id: string,
  instructions: string
): Promise<Project> =>
  invoke<Project>("project_update_instructions", { id, instructions });

/**
 * Deletes the project, its saved chats and its temporary chats; resolves
 * with how many saved chats were deleted.
 */
const projectDelete = (id: string): Promise<number> =>
  invoke<number>("project_delete", { id });

/**
 * Off stops the project's background summaries; on queues its pending chats
 * at once.
 */
const projectSetAutoMemory = (id: string, on: boolean): Promise<Project> =>
  invoke<Project>("project_set_auto_memory", { id, on });

/** The project's memory entries, and its chats that have none yet. */
const projectMemoryList = (projectId: string): Promise<MemoryList> =>
  invoke<MemoryList>("project_memory_list", { projectId });

/** Moves the project's pending chats to the front of the background queue. */
const projectMemoryCatchUp = (projectId: string): Promise<void> =>
  invoke<undefined>("project_memory_catch_up", { projectId });

/** Pending chats per project with automatic memory on; zeros left out. */
const projectMemoryPending = (): Promise<PendingCount[]> =>
  invoke<PendingCount[]>("project_memory_pending");

/**
 * Calls `handler` for every `memory-status` event; resolves with the
 * function that stops listening.
 */
const onMemoryStatus = (
  handler: (status: MemoryStatus) => void
): Promise<() => void> =>
  listen<MemoryStatus>("memory-status", (event) => {
    handler(event.payload);
  });

/** Replaces a chat's entry; its next summary starts from this text. */
const projectMemoryUpdate = (
  conversationId: string,
  content: string
): Promise<MemoryEntry> =>
  invoke<MemoryEntry>("project_memory_update", { conversationId, content });

const projectMemoryDelete = (conversationId: string): Promise<void> =>
  invoke<undefined>("project_memory_delete", { conversationId });

/**
 * Summarizes one chat, or every saved chat of the project when
 * `conversationId` is `null`, each with its own model. Each request is
 * audited as `ollama.memory`.
 */
const projectMemoryRefresh = (
  projectId: string,
  conversationId: string | null
): Promise<RefreshResult[]> =>
  invoke<RefreshResult[]>("project_memory_refresh", {
    projectId,
    conversationId,
  });

/**
 * The exact `system` message the project's chats get, or with
 * `conversationId` what that chat gets; with `draft` too, the passages of the
 * project's files found for it. Sends nothing to Ollama.
 */
const projectContextPreview = (
  projectId: string,
  conversationId: string | null,
  draft: string | null
): Promise<ContextPreview> =>
  invoke<ContextPreview>("project_context_preview", {
    projectId,
    conversationId,
    draft,
  });

/**
 * Stores one file's text in the project, replacing a file of the same name.
 * The webview read it; Rust never reads a disk path.
 */
const projectFileAdd = (
  projectId: string,
  name: string,
  content: string
): Promise<ProjectFile> =>
  invoke<ProjectFile>("project_file_add", { projectId, name, content });

/**
 * Sends a PDF's bytes for Rust to extract its text, stored under `name` like
 * a text file; the PDF itself is not kept. The bytes go as the raw body, the
 * name percent-encoded in a header, since header values are not Unicode.
 */
const projectFileAddPdf = (
  projectId: string,
  name: string,
  bytes: ArrayBuffer
): Promise<ProjectFile> =>
  invoke<ProjectFile>("project_file_add_pdf", new Uint8Array(bytes), {
    headers: {
      "x-project-id": projectId,
      "x-file-name": encodeURIComponent(name),
    },
  });

/** `limit` of the project's files by name, from `offset`; at most 500. */
const projectFileList = (
  projectId: string,
  offset: number,
  limit: number
): Promise<FilePage> =>
  invoke<FilePage>("project_file_list", { projectId, offset, limit });

const projectFileDelete = (projectId: string, id: string): Promise<void> =>
  invoke<undefined>("project_file_delete", { projectId, id });

/** Rust sends the chat's saved settings: system prompt, options and Think. */
type ChatTurnArgs = { model: string };

type ChatSendArgs = ChatTurnArgs & { conversationId: string; prompt: string };

/**
 * Saves the prompt and starts a streamed reply. Resolves with the request
 * and message ids as soon as Rust has admitted the request; events may
 * arrive before it resolves. Rust builds the history from the saved
 * conversation.
 */
const chatSend = (
  { conversationId, model, prompt }: ChatSendArgs,
  onEvent: (event: ChatEvent) => void
): Promise<SendStarted> =>
  invoke<SendStarted>("chat_send", {
    conversationId,
    model,
    prompt,
    onEvent: new Channel<ChatEvent>(onEvent),
  });

type ChatRegenerateArgs = ChatTurnArgs & { conversationId: string };

/**
 * Like `chatSend`, but streams a new reply to the last prompt. The old reply
 * is deleted only once the new one completes.
 */
const chatRegenerate = (
  { conversationId, model }: ChatRegenerateArgs,
  onEvent: (event: ChatEvent) => void
): Promise<SendStarted> =>
  invoke<SendStarted>("chat_regenerate", {
    conversationId,
    model,
    onEvent: new Channel<ChatEvent>(onEvent),
  });

type ChatEditArgs = ChatSendArgs & { messageId: string };

/**
 * Like `chatSend`, but replaces the user message `messageId` and everything
 * after it. Nothing is deleted until the new reply completes.
 */
const chatEdit = (
  { conversationId, messageId, model, prompt }: ChatEditArgs,
  onEvent: (event: ChatEvent) => void
): Promise<SendStarted> =>
  invoke<SendStarted>("chat_edit", {
    conversationId,
    messageId,
    model,
    prompt,
    onEvent: new Channel<ChatEvent>(onEvent),
  });

const chatCancel = (requestId: RequestId): Promise<void> =>
  invoke<undefined>("chat_cancel", { requestId });

type ChatCompareArgs = {
  conversationId: string;
  prompt: string;
  /** Two or three different models, answered in this order. */
  models: string[];
};

/**
 * Saves the prompt and has each model answer it in turn, never two at once.
 * Resolves once Rust has admitted it; events may arrive before it resolves.
 */
const chatCompare = (
  { conversationId, prompt, models }: ChatCompareArgs,
  onEvent: (event: CompareEvent) => void
): Promise<CompareStarted> =>
  invoke<CompareStarted>("chat_compare", {
    conversationId,
    prompt,
    models,
    onEvent: new Channel<CompareEvent>(onEvent),
  });

/** Keeps the complete candidate `messageId`; the chat continues with its model. */
const chatComparePick = (
  conversationId: string,
  messageId: string
): Promise<void> =>
  invoke<undefined>("chat_compare_pick", { conversationId, messageId });

/** Deletes the compared prompt and every candidate. */
const chatCompareDiscard = (
  conversationId: string,
  group: string
): Promise<void> =>
  invoke<undefined>("chat_compare_discard", { conversationId, group });

/**
 * A temporary chat kept only in Rust's memory, with `settings` from its first
 * send on, in project `projectId` unless it is `null`; resolves with its id.
 */
const tempChatStart = (
  settings: ConversationSettings,
  projectId: string | null
): Promise<string> =>
  invoke<string>("temp_chat_start", { settings, projectId });

/** Applies from the temporary chat's next send. */
const tempChatUpdateSettings = (
  sessionId: string,
  settings: ConversationSettings
): Promise<void> =>
  invoke<undefined>("temp_chat_update_settings", { sessionId, settings });

type TempChatSendArgs = ChatTurnArgs & { sessionId: string; prompt: string };

/**
 * Like `chatSend`, but nothing is saved: Rust builds the history from the
 * temporary chat in memory.
 */
const tempChatSend = (
  { sessionId, model, prompt }: TempChatSendArgs,
  onEvent: (event: ChatEvent) => void
): Promise<SendStarted> =>
  invoke<SendStarted>("temp_chat_send", {
    sessionId,
    model,
    prompt,
    onEvent: new Channel<ChatEvent>(onEvent),
  });

/** Like `chatRegenerate`, for a temporary chat. */
const tempChatRegenerate = (
  { sessionId, model }: ChatTurnArgs & { sessionId: string },
  onEvent: (event: ChatEvent) => void
): Promise<SendStarted> =>
  invoke<SendStarted>("temp_chat_regenerate", {
    sessionId,
    model,
    onEvent: new Channel<ChatEvent>(onEvent),
  });

/** Like `chatEdit`, for a temporary chat. */
const tempChatEdit = (
  {
    sessionId,
    messageId,
    model,
    prompt,
  }: TempChatSendArgs & {
    messageId: string;
  },
  onEvent: (event: ChatEvent) => void
): Promise<SendStarted> =>
  invoke<SendStarted>("temp_chat_edit", {
    sessionId,
    messageId,
    model,
    prompt,
    onEvent: new Channel<ChatEvent>(onEvent),
  });

/** Forgets the temporary chat and cancels its reply, if one streams. */
const tempChatDiscard = (sessionId: string): Promise<void> =>
  invoke<undefined>("temp_chat_discard", { sessionId });

/** Newest first; `before` is the id of the last row already shown. */
const activityList = (
  limit: number,
  before: number | null = null
): Promise<AuditEvent[]> =>
  invoke<AuditEvent[]>("activity_list", { limit, before });

/** Every host in the retained audit rows, sorted. */
const activityHosts = (): Promise<string[]> =>
  invoke<string[]>("activity_hosts");

/** Per-model speed from the retained chat and memory audit rows. */
const activityModelStats = (): Promise<ModelStats[]> =>
  invoke<ModelStats[]>("activity_model_stats");

const settingsGet = (): Promise<Settings> => invoke<Settings>("settings_get");

const settingsSet = (patch: SettingsPatch): Promise<Settings> =>
  invoke<Settings>("settings_set", { patch });

export {
  activityHosts,
  activityList,
  activityModelStats,
  appInfo,
  chatCancel,
  chatCompare,
  chatCompareDiscard,
  chatComparePick,
  chatEdit,
  chatRegenerate,
  chatSend,
  conversationCreate,
  conversationDelete,
  conversationGet,
  conversationList,
  conversationMarkdown,
  conversationRename,
  conversationSearch,
  conversationUpdateSettings,
  ollamaListModels,
  ollamaModelCapabilities,
  ollamaStatus,
  onMemoryStatus,
  projectContextPreview,
  projectCreate,
  projectDelete,
  projectFileAdd,
  projectFileAddPdf,
  projectFileDelete,
  projectFileList,
  projectList,
  projectMemoryCatchUp,
  projectMemoryDelete,
  projectMemoryList,
  projectMemoryPending,
  projectMemoryRefresh,
  projectMemoryUpdate,
  projectRename,
  projectSetAutoMemory,
  projectUpdateInstructions,
  settingsGet,
  settingsSet,
  storageStatus,
  tempChatDiscard,
  tempChatEdit,
  tempChatRegenerate,
  tempChatSend,
  tempChatStart,
  tempChatUpdateSettings,
};
