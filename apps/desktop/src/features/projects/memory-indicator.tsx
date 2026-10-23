import { Brain, Check, TriangleAlert } from "lucide-react";

import type { ProjectMemoryStatus } from "./use-memory-status";

const pendingLabel = (count: number): string =>
  count === 1 ? "1 memory pending" : `${count} memories pending`;

type MemoryIndicatorProps = {
  state: ProjectMemoryStatus["state"];
  /** Shown as a badge when above zero. */
  pending?: number;
  onOpenActivity: () => void;
};

/**
 * A `Brain` while a summary is written, a check once memory is updated, a
 * warning opening Activity when it could not be, and the pending count.
 */
const MemoryIndicator = ({
  state,
  pending = 0,
  onOpenActivity,
}: MemoryIndicatorProps) => {
  if (state === null && pending === 0) {
    return null;
  }
  return (
    <span className="flex shrink-0 items-center gap-1 text-xs">
      {state === "running" && (
        <span
          role="status"
          aria-label="Saving to project memory…"
          title="Saving to project memory…"
          className="text-sky-600 dark:text-sky-400"
        >
          <Brain aria-hidden="true" className="size-3.5 animate-pulse" />
        </span>
      )}
      {state === "saved" && (
        <span
          role="status"
          aria-label="Memory updated"
          title="Memory updated"
          className="text-emerald-600 dark:text-emerald-400"
        >
          <Check aria-hidden="true" className="size-3.5" />
        </span>
      )}
      {state === "failed" && (
        <button
          type="button"
          onClick={onOpenActivity}
          aria-label="Couldn't update memory. Open Activity"
          title="Couldn't update memory"
          className="rounded text-amber-600 hover:bg-zinc-100 dark:text-amber-400 dark:hover:bg-zinc-800"
        >
          <TriangleAlert aria-hidden="true" className="size-3.5" />
        </button>
      )}
      {pending > 0 && (
        <span
          role="img"
          aria-label={pendingLabel(pending)}
          title={pendingLabel(pending)}
          className="flex items-center gap-0.5 rounded-full bg-zinc-200 px-1.5 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
        >
          <Brain aria-hidden="true" className="size-3" />
          {pending}
        </span>
      )}
    </span>
  );
};

export { MemoryIndicator };
