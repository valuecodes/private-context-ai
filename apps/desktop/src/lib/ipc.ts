// The only module that imports `@tauri-apps/api`. Everything else calls these
// typed wrappers, so `vi.mock("~/lib/ipc")` is the single test seam.
import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AppInfo,
  ChatEvent,
  ChatMessage,
  ModelInfo,
  OllamaStatus,
  RequestId,
} from "~/lib/types";

const appInfo = (): Promise<AppInfo> => invoke<AppInfo>("app_info");

const ollamaStatus = (): Promise<OllamaStatus> =>
  invoke<OllamaStatus>("ollama_status");

const ollamaListModels = (): Promise<ModelInfo[]> =>
  invoke<ModelInfo[]>("ollama_list_models");

type ChatSendArgs = {
  conversationId: string;
  model: string;
  messages: ChatMessage[];
};

/**
 * Starts a streamed reply. Resolves with the request id as soon as Rust has
 * admitted the request; events may arrive before it resolves.
 */
const chatSend = (
  { conversationId, model, messages }: ChatSendArgs,
  onEvent: (event: ChatEvent) => void
): Promise<RequestId> => {
  const channel = new Channel<ChatEvent>(onEvent);
  return invoke<RequestId>("chat_send", {
    conversationId,
    model,
    messages,
    onEvent: channel,
  });
};

const chatCancel = (requestId: RequestId): Promise<void> =>
  invoke<undefined>("chat_cancel", { requestId });

export { appInfo, chatCancel, chatSend, ollamaListModels, ollamaStatus };
