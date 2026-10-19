import { useEffect, useRef, useState } from "react";

type CopyButtonProps = {
  readonly text: string;
};

type CopyStatus = "idle" | "copied" | "failed";

const resetDelayMs = 1500;

const labels: Record<CopyStatus, string> = {
  idle: "Copy",
  copied: "Copied",
  failed: "Select to copy",
};

// `role="status"` is a polite live region: screen readers read these out when
// they change, without moving focus.
const announcements: Record<CopyStatus, string> = {
  idle: "",
  copied: "Copied to clipboard",
  failed: "Copy failed. Select the command to copy it manually.",
};

const CopyButton = ({ text }: CopyButtonProps) => {
  const [status, setStatus] = useState<CopyStatus>("idle");
  // One pending reset at a time, so a quick second click is not cut short by
  // the first click's timer.
  const resetTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => () => clearTimeout(resetTimer.current), []);

  const copy = () => {
    // Inside the promise chain so a missing Clipboard API (non-secure context)
    // lands in `.catch` instead of throwing out of the click handler.
    void Promise.resolve()
      .then(() => navigator.clipboard.writeText(text))
      .then(() => setStatus("copied"))
      // Clipboard access can be denied; the command text stays selectable.
      .catch(() => setStatus("failed"))
      .finally(() => {
        clearTimeout(resetTimer.current);
        resetTimer.current = setTimeout(() => setStatus("idle"), resetDelayMs);
      });
  };

  return (
    <>
      <button
        type="button"
        onClick={copy}
        // Stable name: a changing name is not reliably announced, so the result
        // goes through the status region below instead.
        aria-label={`Copy: ${text}`}
        className="shrink-0 rounded-md border border-zinc-300 px-2.5 py-1 text-xs font-medium text-zinc-600 transition hover:border-zinc-400 hover:text-zinc-900 focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:outline-none dark:border-zinc-700 dark:text-zinc-400 dark:hover:border-zinc-500 dark:hover:text-zinc-100"
      >
        {labels[status]}
      </button>
      <span role="status" className="sr-only">
        {announcements[status]}
      </span>
    </>
  );
};

export { CopyButton };
