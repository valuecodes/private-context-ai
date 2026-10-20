import type { UiMessage } from "~/features/chat/use-chat-stream";

const STATUS_NOTE: Record<UiMessage["status"], string> = {
  complete: "",
  streaming: "",
  cancelled: "Stopped",
  error: "Failed",
};

const bubbleClass = (message: UiMessage): string =>
  message.role === "user"
    ? "self-end bg-zinc-200 dark:bg-zinc-800"
    : "self-start bg-white ring-1 ring-zinc-200 dark:bg-zinc-900 dark:ring-zinc-800";

const MessageList = ({ messages }: { messages: UiMessage[] }) => {
  if (messages.length === 0) {
    return (
      <p className="m-auto text-sm text-zinc-500 dark:text-zinc-400">
        Pick a model and send a message to start.
      </p>
    );
  }
  return (
    <ol aria-label="Messages" className="flex flex-col gap-3">
      {messages.map((message) => {
        const note = STATUS_NOTE[message.status];
        const waiting =
          message.status === "streaming" && message.content === "";
        return (
          <li
            key={message.id}
            className={`max-w-[80%] rounded-lg px-3 py-2 text-sm whitespace-pre-wrap ${bubbleClass(message)}`}
          >
            <span className="sr-only">
              {message.role === "user" ? "You: " : "Assistant: "}
            </span>
            {waiting ? "…" : message.content}
            {note !== "" && (
              <span className="mt-1 block text-xs text-zinc-500 dark:text-zinc-400">
                {note}
              </span>
            )}
          </li>
        );
      })}
    </ol>
  );
};

export { MessageList };
