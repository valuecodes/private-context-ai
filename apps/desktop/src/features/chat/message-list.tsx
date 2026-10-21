import { Check, Copy } from "lucide-react";
import { memo, useDeferredValue, useState } from "react";

import { Markdown } from "~/features/chat/markdown";
import { TypingIndicator } from "~/features/chat/typing-indicator";
import type { UiMessage } from "~/features/chat/use-chat-stream";
import { copyText } from "~/lib/clipboard";

const STATUS_NOTE: Record<UiMessage["status"], string> = {
  complete: "",
  streaming: "",
  cancelled: "Stopped",
  error: "Failed",
};

const COPIED_MS = 1500;

const CopyMessage = ({ text }: { text: string }) => {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      onClick={() => {
        void copyText(text).then((ok) => {
          if (ok) {
            setCopied(true);
            setTimeout(() => {
              setCopied(false);
            }, COPIED_MS);
          }
        });
      }}
      aria-label={copied ? "Copied" : "Copy message"}
      title="Copy message"
      className="rounded p-1 text-zinc-500 hover:bg-zinc-200 dark:text-zinc-400 dark:hover:bg-zinc-800"
    >
      {copied ? (
        <Check aria-hidden="true" className="size-4" />
      ) : (
        <Copy aria-hidden="true" className="size-4" />
      )}
    </button>
  );
};

/**
 * Past this, a streaming reply is shown as plain text until it ends: each
 * Markdown parse is synchronous and covers the whole reply.
 */
const STREAM_MARKDOWN_MAX_CHARS = 16 * 1024;

const AssistantReply = ({ message }: { message: UiMessage }) => {
  // Re-parsing the Markdown on every token is low priority: React drops
  // stale values, so a short reply never blocks typing or Stop.
  const text = useDeferredValue(message.content);
  const streaming = message.status === "streaming";
  if (streaming && message.content === "") {
    return <TypingIndicator />;
  }
  if (streaming && text.length > STREAM_MARKDOWN_MAX_CHARS) {
    return <p className="whitespace-pre-wrap">{text}</p>;
  }
  return <Markdown text={text} highlight={!streaming} />;
};

/**
 * One message. Memoized: the reducer keeps unchanged messages as the same
 * objects, so only the streaming one re-renders per token.
 */
const MessageItem = memo(({ message }: { message: UiMessage }) => {
  const note = STATUS_NOTE[message.status];
  const user = message.role === "user";
  const waiting = message.status === "streaming" && message.content === "";
  return (
    <li
      className={`group flex flex-col ${user ? "items-end" : "items-stretch"}`}
    >
      <div
        className={
          user
            ? "max-w-[80%] rounded-2xl bg-zinc-200 px-4 py-2 text-sm whitespace-pre-wrap dark:bg-zinc-800"
            : "text-sm"
        }
      >
        <span className="sr-only">{user ? "You: " : "Assistant: "}</span>
        {user ? message.content : <AssistantReply message={message} />}
      </div>
      {note !== "" && (
        <span className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">
          {note}
        </span>
      )}
      {!waiting && (
        <div className="mt-1 flex opacity-0 group-hover:opacity-100 focus-within:opacity-100">
          <CopyMessage text={message.content} />
        </div>
      )}
    </li>
  );
});
MessageItem.displayName = "MessageItem";

const MessageList = ({ messages }: { messages: UiMessage[] }) => {
  if (messages.length === 0) {
    return null;
  }
  return (
    <ol aria-label="Messages" className="flex flex-col gap-4">
      {messages.map((message) => (
        <MessageItem key={message.id} message={message} />
      ))}
    </ol>
  );
};

export { MessageList, STREAM_MARKDOWN_MAX_CHARS };
