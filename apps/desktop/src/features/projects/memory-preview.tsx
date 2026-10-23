import { Copy, X } from "lucide-react";
import { useEffect, useState } from "react";

import { toAppError } from "~/lib/app-error";
import { copyText } from "~/lib/clipboard";
import { projectContextPreview } from "~/lib/ipc";
import type { AppError, ContextPreview } from "~/lib/types";

const button =
  "flex items-center gap-1 rounded border border-zinc-300 px-2 py-1 text-sm hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";

const STATE_NOTE: Record<ContextPreview["entries"][number]["state"], string> = {
  included: "Sent",
  dropped: "Over the budget",
  stale: "Out of date",
};

/** `1234` → `1.2 KB`. */
const formatBytes = (bytes: number): string =>
  bytes < 1000 ? `${bytes} B` : `${(bytes / 1000).toFixed(1)} KB`;

type PreviewBodyProps = {
  preview: ContextPreview | null;
  error: AppError | null;
  /** "What this chat sees" rather than the whole project's memory. */
  forChat: boolean;
  copied: boolean | null;
  onCopy: () => void;
};

/** The assembled `system` message, its entries and the budget meter. */
const PreviewBody = ({
  preview,
  error,
  forChat,
  copied,
  onCopy,
}: PreviewBodyProps) => {
  if (error !== null) {
    return (
      <p role="alert" className="text-sm text-red-600 dark:text-red-400">
        {error.message}
      </p>
    );
  }
  if (preview === null) {
    return <p className="text-sm text-zinc-500">Loading…</p>;
  }
  return (
    <div className="flex min-h-0 flex-col gap-3">
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        {forChat
          ? "The system message this chat's next request starts with, as the chat is now. Its own memory is never sent to it."
          : "The project's instructions and every memory entry. Each chat leaves out its own entry and may have a smaller budget."}
      </p>
      <pre className="max-h-80 min-h-16 overflow-auto rounded border border-zinc-200 bg-zinc-50 p-2 font-mono text-xs whitespace-pre-wrap dark:border-zinc-800 dark:bg-zinc-950">
        {preview.text === "" ? "(no system message)" : preview.text}
      </pre>
      <div className="flex items-center gap-2 text-xs">
        <label htmlFor="memory-budget">Memory</label>
        <meter
          id="memory-budget"
          min={0}
          max={Math.max(preview.budgetBytes, 1)}
          value={preview.memoryBytes}
          className="flex-1"
        />
        <span>{`${formatBytes(preview.memoryBytes)} of ${formatBytes(preview.budgetBytes)}`}</span>
      </div>
      {preview.entries.length > 0 && (
        <ul
          aria-label="Memory entries"
          className="flex flex-col gap-0.5 text-sm"
        >
          {preview.entries.map((entry) => (
            <li
              key={entry.conversationId}
              className={`flex gap-2 ${entry.state === "included" ? "" : "text-zinc-400 line-through dark:text-zinc-600"}`}
            >
              <span className="min-w-0 flex-1 truncate">{entry.title}</span>
              <span className="text-xs no-underline">
                {STATE_NOTE[entry.state]}
              </span>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onCopy}
          disabled={preview.text === ""}
          className={button}
        >
          <Copy aria-hidden="true" className="size-4" />
          Copy
        </button>
        {copied !== null && (
          <span role="status" className="text-xs text-zinc-500">
            {copied ? "Copied" : "Could not copy"}
          </span>
        )}
      </div>
    </div>
  );
};

type MemoryPreviewProps = {
  projectId: string;
  /** "What this chat sees" for this saved chat; the whole project if `null`. */
  conversationId: string | null;
  onClose: () => void;
};

/** A read-only dialog showing exactly what the model is sent. */
const MemoryPreview = ({
  projectId,
  conversationId,
  onClose,
}: MemoryPreviewProps) => {
  const [preview, setPreview] = useState<ContextPreview | null>(null);
  const [failure, setFailure] = useState<AppError | null>(null);
  const [copied, setCopied] = useState<boolean | null>(null);

  useEffect(() => {
    let cancelled = false;
    void projectContextPreview(projectId, conversationId).then(
      (loaded) => {
        if (!cancelled) {
          setPreview(loaded);
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          setFailure(toAppError(error));
        }
      }
    );
    return () => {
      cancelled = true;
    };
  }, [projectId, conversationId]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    globalThis.addEventListener("keydown", onKeyDown);
    return () => {
      globalThis.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  const title =
    conversationId === null ? "Full project memory" : "What this chat sees";
  return (
    <div className="fixed inset-0 z-20 flex items-center justify-center bg-black/40 p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="memory-preview-title"
        className="flex max-h-full w-full max-w-2xl flex-col gap-3 rounded border border-zinc-200 bg-white p-4 shadow-lg dark:border-zinc-700 dark:bg-zinc-900"
      >
        <div className="flex items-center gap-2">
          <h2 id="memory-preview-title" className="flex-1 font-semibold">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            ref={(element) => {
              element?.focus();
            }}
            className={button}
          >
            <X aria-hidden="true" className="size-4" />
          </button>
        </div>
        <PreviewBody
          preview={preview}
          error={failure}
          forChat={conversationId !== null}
          copied={copied}
          onCopy={() => {
            if (preview !== null) {
              void copyText(preview.text).then(setCopied);
            }
          }}
        />
      </div>
    </div>
  );
};

export { MemoryPreview, PreviewBody };
