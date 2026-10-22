import type { CompareState, UiMessage } from "~/features/chat/chat-reducer";
import type { StoredMessage } from "~/lib/types";

/**
 * Saved messages as bubbles. A `pending` row shows as still streaming;
 * replies carry their stats and model.
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
      ...(message.model === null ? {} : { model: message.model }),
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

/**
 * The conversation's bubbles, and the comparison still waiting for a pick:
 * its prompt and candidate replies, which are not part of the conversation
 * yet.
 */
const splitStored = (
  messages: StoredMessage[]
): { messages: UiMessage[]; compare: CompareState | null } => {
  const settled = fromStored(
    messages.filter(({ compareGroup }) => compareGroup === null)
  );
  const grouped = messages.filter(({ compareGroup }) => compareGroup !== null);
  const group = grouped[0]?.compareGroup ?? null;
  const [prompt, ...candidates] = fromStored(grouped);
  if (group === null || prompt?.role !== "user") {
    return { messages: settled, compare: null };
  }
  return { messages: settled, compare: { group, prompt, candidates } };
};

export { fromStored, splitStored };
