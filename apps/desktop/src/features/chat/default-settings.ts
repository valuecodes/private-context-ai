import { THINK_OFF } from "~/features/chat/think-choice";
import type { ConversationSettings } from "~/lib/types";

/**
 * A chat with nothing set: no system prompt, model defaults, thinking and
 * charts off.
 */
const DEFAULT_SETTINGS: ConversationSettings = {
  systemPrompt: "",
  options: {},
  think: THINK_OFF,
  charts: false,
};

export { DEFAULT_SETTINGS };
