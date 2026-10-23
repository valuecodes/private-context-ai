import { Check, Copy, Info, Pencil, RefreshCw } from "lucide-react";
import type { KeyboardEvent, SubmitEvent } from "react";
import {
  Fragment,
  memo,
  useCallback,
  useDeferredValue,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import { ChartBlock, ChartFailed } from "~/features/chat/chart-block";
import { sendsMessage } from "~/features/chat/chat-keys";
import type { UiMessage } from "~/features/chat/chat-reducer";
import { Markdown } from "~/features/chat/markdown";
import { formatStats } from "~/features/chat/message-stats";
import { ThinkingBlock } from "~/features/chat/thinking-block";
import { TypingIndicator } from "~/features/chat/typing-indicator";
import { copyText } from "~/lib/clipboard";
import type { ChartEntry } from "~/lib/types";

const STATUS_NOTE: Record<UiMessage["status"], string> = {
  complete: "",
  streaming: "",
  cancelled: "Stopped",
  error: "Failed",
};

const COPIED_MS = 1500;

const actionButton =
  "rounded p-1 text-zinc-500 hover:bg-zinc-200 disabled:cursor-not-allowed disabled:opacity-40 dark:text-zinc-400 dark:hover:bg-zinc-800";

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
      className={actionButton}
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

const ReplyText = ({ message }: { message: UiMessage }) => {
  // Re-parsing the Markdown on every token is low priority: React drops
  // stale values, so a short reply never blocks typing or Stop.
  const text = useDeferredValue(message.content);
  const streaming = message.status === "streaming";
  if (streaming && message.content === "") {
    // The thinking block already shows that the model is working.
    return (message.thinking ?? "") === "" ? <TypingIndicator /> : null;
  }
  if (streaming && text.length > STREAM_MARKDOWN_MAX_CHARS) {
    return <p className="whitespace-pre-wrap">{text}</p>;
  }
  return <Markdown text={text} highlight={!streaming} />;
};

/** `content` cut at each chart's offset, in offset order: the text before each. */
const chartSegments = (content: string, charts: ChartEntry[]) => {
  const sorted = charts.toSorted((a, b) => a.offset - b.offset);
  const segments: { text: string; entry: ChartEntry }[] = [];
  let from = 0;
  for (const entry of sorted) {
    const to = Math.max(from, entry.offset);
    segments.push({ text: content.slice(from, to), entry });
    from = to;
  }
  return { segments, rest: content.slice(from) };
};

/**
 * A reply that called `render_chart`: the text before each chart, the chart
 * (or a note when Rust refused it), then the rest, which may still stream.
 */
const ChartedReply = ({
  message,
  charts,
}: {
  message: UiMessage;
  charts: ChartEntry[];
}) => {
  const highlight = message.status !== "streaming";
  const { segments, rest } = chartSegments(message.content, charts);
  return (
    <>
      {segments.map(({ text, entry }, index) => (
        // Offsets can repeat; the order is fixed once a chart arrives.
        <Fragment key={index}>
          {text !== "" && <Markdown text={text} highlight={highlight} />}
          {entry.spec === null ? (
            <ChartFailed reason={entry.error} />
          ) : (
            <ChartBlock spec={entry.spec} />
          )}
        </Fragment>
      ))}
      <ReplyText message={{ ...message, content: rest }} />
    </>
  );
};

const AssistantReply = ({ message }: { message: UiMessage }) => {
  const thinking = message.thinking ?? "";
  const streaming = message.status === "streaming";
  const charts = message.charts ?? [];
  return (
    <>
      {thinking !== "" && (
        <ThinkingBlock
          text={thinking}
          ms={message.thinkingMs ?? null}
          active={streaming && message.content === "" && charts.length === 0}
        />
      )}
      {charts.length === 0 ? (
        <ReplyText message={message} />
      ) : (
        <ChartedReply message={message} charts={charts} />
      )}
    </>
  );
};

/** Which model wrote a reply. */
const ModelLabel = ({ model }: { model: string }) => (
  <span className="mb-1 block text-xs text-zinc-500 dark:text-zinc-400">
    {model}
  </span>
);

type MessageEditorProps = {
  text: string;
  busy: boolean;
  /** Returns `false` when the resend was refused, keeping the editor open. */
  onSave: (text: string) => boolean;
  onCancel: () => void;
};

/** Edits a sent message in place; saving resends it. */
const MessageEditor = ({
  text,
  busy,
  onSave,
  onCancel,
}: MessageEditorProps) => {
  const [draft, setDraft] = useState(text);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const id = useId();
  const changed = draft.trim();
  const savable = changed !== "" && !busy;

  useLayoutEffect(() => {
    textarea.current?.focus();
  }, []);

  const save = () => {
    if (savable && onSave(changed)) {
      onCancel();
    }
  };

  const onSubmit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    save();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const isComposing = event.nativeEvent.isComposing;
    if (event.key === "Escape" && !isComposing) {
      // The chat view's Esc would also stop a streaming reply.
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    } else if (
      sendsMessage({ key: event.key, shiftKey: event.shiftKey, isComposing })
    ) {
      event.preventDefault();
      save();
    }
  };

  return (
    <form
      onSubmit={onSubmit}
      className="flex w-full max-w-[80%] flex-col gap-2"
    >
      <label htmlFor={`${id}-text`} className="sr-only">
        Edit message
      </label>
      <textarea
        id={`${id}-text`}
        ref={textarea}
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value);
        }}
        onKeyDown={onKeyDown}
        rows={3}
        aria-describedby={`${id}-note`}
        className="w-full resize-y rounded-2xl border border-zinc-300 bg-white px-4 py-2 text-sm outline-none focus:ring-2 focus:ring-zinc-400 dark:border-zinc-700 dark:bg-zinc-900"
      />
      <div className="flex items-center justify-end gap-2">
        <span
          id={`${id}-note`}
          className="mr-auto text-xs text-zinc-500 dark:text-zinc-400"
        >
          Later messages are removed
        </span>
        <button
          type="button"
          onClick={onCancel}
          className="rounded-full border border-zinc-300 px-3 py-1 text-sm hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={!savable}
          className="rounded-full bg-zinc-900 px-3 py-1 text-sm text-white hover:bg-zinc-700 disabled:cursor-not-allowed disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
        >
          Save & resend
        </button>
      </div>
    </form>
  );
};

type MessageItemProps = {
  message: UiMessage;
  /** A reply streams or no model is picked: nothing can be resent. */
  busy: boolean;
  /** The last message, so its reply can be regenerated. */
  last: boolean;
  onRegenerate: () => void;
  onEdit: (messageId: string, text: string) => boolean;
};

/**
 * One message. Memoized: the reducer keeps unchanged messages as the same
 * objects and the callbacks are stable, so only the streaming one
 * re-renders per token.
 */
const MessageItem = memo(
  ({ message, busy, last, onRegenerate, onEdit }: MessageItemProps) => {
    const [editing, setEditing] = useState(false);
    const note = STATUS_NOTE[message.status];
    const user = message.role === "user";
    // No copy button until there is an answer to copy; thinking is not
    // copied.
    const waiting = message.status === "streaming" && message.content === "";
    // A failed prompt was never saved, so there is nothing to replace; a
    // new one is editable once its saved id has arrived.
    const editable =
      user && message.status !== "error" && message.local !== true;
    const stats =
      message.status === "complete" && message.stats !== undefined
        ? formatStats(message.stats)
        : null;
    if (editing) {
      return (
        <li className="flex flex-col items-end">
          <MessageEditor
            text={message.content}
            busy={busy}
            onSave={(text) => onEdit(message.id, text)}
            onCancel={() => {
              setEditing(false);
            }}
          />
        </li>
      );
    }
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
          {!user && message.model !== undefined && (
            <ModelLabel model={message.model} />
          )}
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
            {editable && (
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  setEditing(true);
                }}
                aria-label="Edit message"
                title="Edit message"
                className={actionButton}
              >
                <Pencil aria-hidden="true" className="size-4" />
              </button>
            )}
            {last && !user && (
              <button
                type="button"
                disabled={busy}
                onClick={onRegenerate}
                aria-label="Regenerate reply"
                title="Regenerate reply"
                className={actionButton}
              >
                <RefreshCw aria-hidden="true" className="size-4" />
              </button>
            )}
            {stats !== null && (
              <span
                role="img"
                aria-label={stats}
                title={stats}
                className="p-1 text-zinc-500 dark:text-zinc-400"
              >
                <Info aria-hidden="true" className="size-4" />
              </span>
            )}
          </div>
        )}
      </li>
    );
  }
);
MessageItem.displayName = "MessageItem";

type MessageListProps = {
  messages: UiMessage[];
  busy: boolean;
  onRegenerate: () => void;
  /** Returns `false` when the resend was refused. */
  onEdit: (messageId: string, text: string) => boolean;
};

const MessageList = ({
  messages,
  busy,
  onRegenerate,
  onEdit,
}: MessageListProps) => {
  // The handlers change every render; the items get stable wrappers that
  // call the latest ones, so a token does not re-render every message.
  const handlers = useRef({ onRegenerate, onEdit });
  useLayoutEffect(() => {
    handlers.current = { onRegenerate, onEdit };
  });
  const regenerate = useCallback(() => {
    handlers.current.onRegenerate();
  }, []);
  const edit = useCallback(
    (messageId: string, text: string) =>
      handlers.current.onEdit(messageId, text),
    []
  );
  if (messages.length === 0) {
    return null;
  }
  return (
    <ol aria-label="Messages" className="flex flex-col gap-4">
      {messages.map((message, index) => (
        <MessageItem
          key={message.id}
          message={message}
          busy={busy}
          last={index === messages.length - 1}
          onRegenerate={regenerate}
          onEdit={edit}
        />
      ))}
    </ol>
  );
};

export { AssistantReply, MessageList, STREAM_MARKDOWN_MAX_CHARS };
