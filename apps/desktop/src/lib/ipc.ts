// The only module that imports `@tauri-apps/api`. Everything else calls these
// typed wrappers, so `vi.mock("~/lib/ipc")` is the single test seam.
import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AppInfo,
  AuditEvent,
  ChatEvent,
  Conversation,
  ConversationWithMessages,
  ModelInfo,
  OllamaStatus,
  RequestId,
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

const conversationCreate = (
  title: string | null,
  model: string
): Promise<Conversation> =>
  invoke<Conversation>("conversation_create", { title, model });

const conversationList = (): Promise<Conversation[]> =>
  invoke<Conversation[]>("conversation_list");

const conversationGet = (id: string): Promise<ConversationWithMessages> =>
  invoke<ConversationWithMessages>("conversation_get", { id });

const conversationRename = (id: string, title: string): Promise<Conversation> =>
  invoke<Conversation>("conversation_rename", { id, title });

const conversationDelete = (id: string): Promise<void> =>
  invoke<undefined>("conversation_delete", { id });

type ChatSendArgs = {
  conversationId: string;
  model: string;
  prompt: string;
};

/**
 * Saves the prompt and starts a streamed reply. Resolves with the request id
 * as soon as Rust has admitted the request; events may arrive before it
 * resolves. Rust builds the history from the saved conversation.
 */
const chatSend = (
  { conversationId, model, prompt }: ChatSendArgs,
  onEvent: (event: ChatEvent) => void
): Promise<RequestId> => {
  const channel = new Channel<ChatEvent>(onEvent);
  return invoke<RequestId>("chat_send", {
    conversationId,
    model,
    prompt,
    onEvent: channel,
  });
};

const chatCancel = (requestId: RequestId): Promise<void> =>
  invoke<undefined>("chat_cancel", { requestId });

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
  chatSend,
  conversationCreate,
  conversationDelete,
  conversationGet,
  conversationList,
  conversationRename,
  ollamaListModels,
  ollamaStatus,
  settingsGet,
  settingsSet,
  storageStatus,
};
