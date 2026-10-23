import { Eye, Pencil, RefreshCw, Trash } from "lucide-react";
import type { SubmitEvent } from "react";
import { useState } from "react";

import type { MemoryEntry } from "~/lib/types";

import type { MemoryState } from "./use-project-memory";
import { useProjectMemory } from "./use-project-memory";

const button =
  "flex items-center gap-1 rounded border border-zinc-300 px-2 py-1 text-sm hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";
const small =
  "flex items-center gap-1 rounded px-1.5 py-0.5 text-xs hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-zinc-800";

/** Local date and time; the raw text if it does not parse. */
const formatTime = (at: string): string => {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleString();
};

/** What the panel says about an entry, most important first. */
const entryNotes = (entry: MemoryEntry): string[] => {
  const notes: string[] = [];
  if (entry.state === "stale") {
    notes.push("Out of date: the chat was edited. Not sent until refreshed");
  } else if (entry.state === "pending") {
    notes.push("New messages not in memory yet");
  }
  if (entry.rebuilt) {
    notes.push("Rebuilt after the chat was edited");
  }
  if (entry.userEdited) {
    notes.push("Edited");
  }
  if (entry.truncated) {
    notes.push("A long message was shortened");
  }
  return notes;
};

type EntryProps = {
  entry: MemoryEntry;
  disabled: boolean;
  refreshing: boolean;
  onRefresh: () => void;
  /** Resolves `false` when the text was refused, keeping the editor open. */
  onUpdate: (content: string) => Promise<boolean>;
  onRemove: () => void;
};

const Entry = ({
  entry,
  disabled,
  refreshing,
  onRefresh,
  onUpdate,
  onRemove,
}: EntryProps) => {
  const [editing, setEditing] = useState(false);
  const notes = entryNotes(entry);
  const editorId = `memory-${entry.conversationId}`;

  const onSubmit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    const content = new FormData(event.currentTarget).get("content");
    if (typeof content === "string") {
      void onUpdate(content).then((saved) => {
        if (saved) {
          setEditing(false);
        }
      });
    }
  };

  return (
    <li className="flex flex-col gap-1 rounded border border-zinc-200 p-2 dark:border-zinc-800">
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm font-medium">
          {entry.title}
        </span>
        <button
          type="button"
          onClick={onRefresh}
          disabled={disabled}
          aria-label={`Refresh memory of ${entry.title}`}
          title="Refresh"
          className={small}
        >
          <RefreshCw
            aria-hidden="true"
            className={`size-3.5 ${refreshing ? "animate-spin" : ""}`}
          />
        </button>
        <button
          type="button"
          onClick={() => {
            setEditing(true);
          }}
          disabled={disabled || editing}
          aria-label={`Edit memory of ${entry.title}`}
          title="Edit"
          className={small}
        >
          <Pencil aria-hidden="true" className="size-3.5" />
        </button>
        <button
          type="button"
          onClick={onRemove}
          disabled={disabled}
          aria-label={`Delete memory of ${entry.title}`}
          title="Delete"
          className={small}
        >
          <Trash aria-hidden="true" className="size-3.5" />
        </button>
      </div>
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        {`Updated ${formatTime(entry.updatedAt)}${entry.model === null ? "" : ` by ${entry.model}`}`}
      </p>
      {notes.length > 0 && (
        <ul className="flex flex-wrap gap-1">
          {notes.map((note) => (
            <li
              key={note}
              className="rounded bg-zinc-100 px-1.5 py-0.5 text-xs text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"
            >
              {note}
            </li>
          ))}
        </ul>
      )}
      {editing ? (
        <form onSubmit={onSubmit} className="flex flex-col gap-1">
          <label htmlFor={editorId} className="sr-only">
            {`Memory of ${entry.title}`}
          </label>
          <textarea
            id={editorId}
            name="content"
            defaultValue={entry.content}
            rows={6}
            maxLength={4096}
            className="resize-y rounded border border-zinc-300 bg-white px-2 py-1 text-sm dark:border-zinc-700 dark:bg-zinc-900"
          />
          <div className="flex gap-2">
            <button type="submit" disabled={disabled} className={button}>
              Save
            </button>
            <button
              type="button"
              onClick={() => {
                setEditing(false);
              }}
              className={button}
            >
              Cancel
            </button>
          </div>
        </form>
      ) : (
        <p className="text-sm whitespace-pre-wrap">{entry.content}</p>
      )}
    </li>
  );
};

type MemoryListProps = {
  state: MemoryState;
  autoMemory: boolean;
  /** True while a saved chat's reply streams: changes wait for it. */
  disabled: boolean;
  /** A chat's title, for refresh failures of chats without an entry. */
  titleOf: (conversationId: string) => string;
  onRefresh: (conversationId: string | null) => void;
  onUpdate: (conversationId: string, content: string) => Promise<boolean>;
  onRemove: (conversationId: string) => void;
  onAutoMemory: (on: boolean) => void;
  onPreview: () => void;
};

/** The memory section of a project: its entries and their controls. */
const MemoryList = ({
  state,
  autoMemory,
  disabled,
  titleOf,
  onRefresh,
  onUpdate,
  onRemove,
  onAutoMemory,
  onPreview,
}: MemoryListProps) => {
  const busy = disabled || state.refreshing !== null;
  return (
    <section aria-labelledby="project-memory" className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <h3 id="project-memory" className="flex-1 font-medium">
          Project memory
        </h3>
        <button
          type="button"
          onClick={() => {
            onRefresh(null);
          }}
          disabled={busy}
          className={button}
        >
          <RefreshCw
            aria-hidden="true"
            className={`size-4 ${state.refreshing === "all" ? "animate-spin" : ""}`}
          />
          {state.refreshing === null ? "Refresh memory" : "Refreshing…"}
        </button>
        <button type="button" onClick={onPreview} className={button}>
          <Eye aria-hidden="true" className="size-4" />
          View full memory
        </button>
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={autoMemory}
          disabled={disabled}
          aria-describedby="auto-memory-hint"
          onChange={(event) => {
            onAutoMemory(event.currentTarget.checked);
          }}
        />
        Automatic memory
      </label>
      <p
        id="auto-memory-hint"
        className="text-xs text-zinc-500 dark:text-zinc-400"
      >
        Each chat&apos;s summary is written by its own model on this computer
        and sent to the project&apos;s other chats as notes, not instructions.
      </p>
      {state.error !== null && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {state.error.message}
        </p>
      )}
      {state.failures.length > 0 && (
        <ul role="alert" className="text-sm text-red-600 dark:text-red-400">
          {state.failures.map((failure) => (
            <li key={failure.conversationId}>
              {`${titleOf(failure.conversationId)}: ${failure.error?.message ?? "not summarized"}`}
            </li>
          ))}
        </ul>
      )}
      {state.phase === "ready" && state.entries.length === 0 && (
        <p className="text-sm text-zinc-500 dark:text-zinc-400">
          Nothing remembered yet
        </p>
      )}
      {state.entries.length > 0 && (
        <ul aria-label="Memory entries" className="flex flex-col gap-2">
          {state.entries.map((entry) => (
            <Entry
              key={entry.conversationId}
              entry={entry}
              disabled={busy}
              refreshing={state.refreshing === entry.conversationId}
              onRefresh={() => {
                onRefresh(entry.conversationId);
              }}
              onUpdate={(content) => onUpdate(entry.conversationId, content)}
              onRemove={() => {
                onRemove(entry.conversationId);
              }}
            />
          ))}
        </ul>
      )}
    </section>
  );
};

type MemoryPanelProps = Pick<
  MemoryListProps,
  "autoMemory" | "disabled" | "titleOf" | "onAutoMemory" | "onPreview"
> & { projectId: string };

/** The memory list of one project, loading and changing its entries. */
const MemoryPanel = ({ projectId, ...props }: MemoryPanelProps) => {
  const memory = useProjectMemory(projectId);
  return (
    <MemoryList
      {...props}
      state={memory.state}
      onRefresh={memory.refresh}
      onUpdate={memory.update}
      onRemove={memory.remove}
    />
  );
};

export { entryNotes, MemoryList, MemoryPanel };
