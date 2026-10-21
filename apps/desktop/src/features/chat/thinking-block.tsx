import { Brain, ChevronRight } from "lucide-react";

/** Whole seconds for the summary, never "0s". */
const thinkingSeconds = (ms: number): number =>
  Math.max(1, Math.round(ms / 1000));

type ThinkingBlockProps = {
  text: string;
  /** Known once the answer started (or from the saved reply). */
  ms: number | null;
  /** Thinking streams and no answer has arrived yet. */
  active: boolean;
};

/**
 * The model's reasoning, as plain text: open while it streams, collapsed once
 * the answer starts. The user can open it again; React only changes `open`
 * when `active` does.
 */
const ThinkingBlock = ({ text, ms, active }: ThinkingBlockProps) => {
  let summary = "Thoughts";
  if (active) {
    summary = "Thinking…";
  } else if (ms !== null) {
    summary = `Thought for ${thinkingSeconds(ms)}s`;
  }
  return (
    <details
      open={active}
      className="group/thinking mb-2 text-sm text-zinc-500 dark:text-zinc-400"
    >
      <summary className="flex w-fit cursor-pointer list-none items-center gap-1.5 rounded py-0.5 select-none hover:text-zinc-900 dark:hover:text-zinc-100 [&::-webkit-details-marker]:hidden">
        <Brain
          aria-hidden="true"
          className={`size-4 ${active ? "motion-safe:animate-pulse" : ""}`}
        />
        {summary}
        <ChevronRight
          aria-hidden="true"
          className="size-4 transition-transform group-open/thinking:rotate-90"
        />
      </summary>
      <p className="mt-1 border-l-2 border-zinc-300 pl-3 whitespace-pre-wrap dark:border-zinc-700">
        {text}
      </p>
    </details>
  );
};

export { ThinkingBlock };
