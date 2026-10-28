import { ChevronRight, FileSearch } from "lucide-react";

import type { FileLookup } from "~/lib/types";

/** `count` with its noun, singular for one. */
const counted = (count: number, noun: string): string =>
  `${count} ${noun}${count === 1 ? "" : "s"}`;

const ACTION: Record<FileLookup["tool"], string> = {
  list: "Listed files",
  search: "Searched",
  read: "Read",
  unknown: "Called an unknown tool",
};

const FOUND: Record<FileLookup["tool"], string> = {
  list: "name",
  search: "passage",
  read: "line",
  unknown: "result",
};

/** One lookup as a line of text: what was looked up and what came back. */
const describe = ({ tool, target, hits, error }: FileLookup): string => {
  const subject = tool === "search" && target !== "" ? `“${target}”` : target;
  const what = subject === "" ? ACTION[tool] : `${ACTION[tool]} ${subject}`;
  return error === undefined
    ? `${what} — ${counted(hits, FOUND[tool])}`
    : `${what} (failed: ${error})`;
};

/**
 * What the model looked up in the project's files before answering, as plain
 * text: collapsed, the user can open it. Model text appears only as text.
 */
const FileLookups = ({ lookups }: { lookups: FileLookup[] }) => (
  <details className="group/lookups mb-2 text-sm text-zinc-500 dark:text-zinc-400">
    <summary className="flex w-fit cursor-pointer list-none items-center gap-1.5 rounded py-0.5 select-none hover:text-zinc-900 dark:hover:text-zinc-100 [&::-webkit-details-marker]:hidden">
      <FileSearch aria-hidden="true" className="size-4" />
      Searched files ({lookups.length})
      <ChevronRight
        aria-hidden="true"
        className="size-4 transition-transform group-open/lookups:rotate-90"
      />
    </summary>
    <ul className="mt-1 border-l-2 border-zinc-300 pl-3 dark:border-zinc-700">
      {lookups.map((lookup, index) => (
        // Lookups only ever grow at the end, so the index is stable.
        <li key={index} className="break-words">
          {describe(lookup)}
        </li>
      ))}
    </ul>
  </details>
);

export { FileLookups };
