import type { UiMessage } from "~/features/chat/chat-reducer";
import type { StoredMessage } from "~/lib/types";

/**
 * Saved messages as bubbles. A `pending` row shows as still streaming;
 * replies carry their stats.
 */
const fromStored = (messages: StoredMessage[]): UiMessage[] =>
  messages.flatMap((message): UiMessage[] => {
    if (message.role === "system") {
      return [];
    }
    const bubble: UiMessage = {
      id: message.id,
      role: message.role,
      content: message.content,
      status: message.status === "pending" ? "streaming" : message.status,
      ...(message.role === "assistant"
        ? {
            stats: {
              completionTokens: message.completionTokens,
              durationMs: message.durationMs,
              evalMs: message.evalMs,
            },
          }
        : {}),
    };
    return message.thinking === null || message.thinking === ""
      ? [bubble]
      : [
          {
            ...bubble,
            thinking: message.thinking,
            thinkingMs: message.thinkingMs,
          },
        ];
  });

export { fromStored };
