// The only module that imports `@tauri-apps/api`. Everything else calls these
// typed wrappers, so `vi.mock("~/lib/ipc")` is the single test seam.
import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AppInfo,
  AuditEvent,
  ChatEvent,
  Conversation,
  ConversationSettings,
  ConversationWithMessages,
  ModelCapabilities,
  ModelInfo,
  OllamaStatus,
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

/** Saved with `settings` at once, so its first send already uses them. */
const conversationCreate = (
  title: string | null,
  model: string,
  settings: ConversationSettings
): Promise<Conversation> =>
  invoke<Conversation>("conversation_create", { title, model, settings });

const conversationList = (): Promise<Conversation[]> =>
  invoke<Conversation[]>("conversation_list");

const conversationGet = (id: string): Promise<ConversationWithMessages> =>
  invoke<ConversationWithMessages>("conversation_get", { id });

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

/**
 * A temporary chat kept only in Rust's memory, with `settings` from its first
 * send on; resolves with its id.
 */
const tempChatStart = (settings: ConversationSettings): Promise<string> =>
  invoke<string>("temp_chat_start", { settings });

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

const settingsGet = (): Promise<Settings> => invoke<Settings>("settings_get");

const settingsSet = (patch: SettingsPatch): Promise<Settings> =>
  invoke<Settings>("settings_set", { patch });

export {
  activityHosts,
  activityList,
  appInfo,
  chatCancel,
  chatEdit,
  chatRegenerate,
  chatSend,
  conversationCreate,
  conversationDelete,
  conversationGet,
  conversationList,
  conversationRename,
  conversationUpdateSettings,
  ollamaListModels,
  ollamaModelCapabilities,
  ollamaStatus,
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
