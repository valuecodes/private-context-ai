import type { KeyboardEvent, SubmitEvent } from "react";
import { useState } from "react";

import type { ConversationsState } from "~/features/conversations/use-conversations";
import type { Conversation } from "~/lib/types";

type ItemMode = "view" | "rename" | "confirm";

type SidebarItemProps = {
  conversation: Conversation;
  selected: boolean;
  /** True while a reply streams: switching and editing wait for it. */
  disabled: boolean;
  mode: ItemMode;
  onSelect: () => void;
  onMode: (mode: ItemMode) => void;
  onRename: (title: string) => void;
  onDelete: () => void;
};

const smallButton =
  "rounded px-1.5 py-0.5 text-xs text-zinc-500 hover:bg-zinc-200 disabled:opacity-40 dark:text-zinc-400 dark:hover:bg-zinc-800";

const SidebarItem = ({
  conversation,
  selected,
  disabled,
  mode,
  onSelect,
  onMode,
  onRename,
  onDelete,
}: SidebarItemProps) => {
  if (mode === "rename") {
    const onSubmit = (event: SubmitEvent<HTMLFormElement>) => {
      event.preventDefault();
      const title = new FormData(event.currentTarget).get("title");
      if (typeof title === "string" && title.trim() !== "") {
        onRename(title);
      }
    };
    const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "Escape") {
        onMode("view");
      }
    };
    return (
      <li>
        <form onSubmit={onSubmit} className="flex items-center gap-1">
          <label className="sr-only" htmlFor={`title-${conversation.id}`}>
            New title
          </label>
          <input
            id={`title-${conversation.id}`}
            name="title"
            defaultValue={conversation.title}
            maxLength={200}
            onKeyDown={onKeyDown}
            ref={(element) => {
              element?.focus();
            }}
            className="min-w-0 flex-1 rounded border border-zinc-300 bg-white px-1.5 py-0.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
          />
          <button type="submit" className={smallButton}>
            Save
          </button>
          <button
            type="button"
            onClick={() => {
              onMode("view");
            }}
            className={smallButton}
          >
            Cancel
          </button>
        </form>
      </li>
    );
  }

  if (mode === "confirm") {
    return (
      <li className="rounded bg-red-50 px-2 py-1 text-sm dark:bg-red-950">
        <p className="truncate">{`Delete “${conversation.title}”?`}</p>
        <div className="mt-1 flex gap-1">
          <button
            type="button"
            onClick={onDelete}
            // A reply may have started since the confirm opened.
            disabled={disabled}
            className="rounded bg-red-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-red-700 disabled:opacity-40"
          >
            Delete
          </button>
          <button
            type="button"
            onClick={() => {
              onMode("view");
            }}
            className={smallButton}
          >
            Keep
          </button>
        </div>
      </li>
    );
  }

  return (
    <li
      className={`group flex items-center gap-1 rounded ${selected ? "bg-zinc-200 dark:bg-zinc-800" : "hover:bg-zinc-100 dark:hover:bg-zinc-900"}`}
    >
      <button
        type="button"
        onClick={onSelect}
        disabled={disabled}
        aria-current={selected ? "true" : undefined}
        className="min-w-0 flex-1 truncate px-2 py-1 text-left text-sm disabled:cursor-not-allowed"
      >
        {conversation.title}
      </button>
      <button
        type="button"
        onClick={() => {
          onMode("rename");
        }}
        disabled={disabled}
        aria-label={`Rename ${conversation.title}`}
        className={smallButton}
      >
        Rename
      </button>
      <button
        type="button"
        onClick={() => {
          onMode("confirm");
        }}
        disabled={disabled}
        aria-label={`Delete ${conversation.title}`}
        className={smallButton}
      >
        Delete
      </button>
    </li>
  );
};

type SidebarProps = {
  state: ConversationsState;
  disabled: boolean;
  version: string;
  onNew: () => void;
  onSelect: (id: string) => void;
  /** Resolves `false` when the title was refused, keeping the editor open. */
  onRename: (id: string, title: string) => Promise<boolean>;
  onDelete: (id: string) => void;
};

const Sidebar = ({
  state,
  disabled,
  version,
  onNew,
  onSelect,
  onRename,
  onDelete,
}: SidebarProps) => {
  // At most one conversation is being renamed or confirmed at a time.
  const [editing, setEditing] = useState<{
    id: string;
    mode: ItemMode;
  } | null>(null);

  const modeOf = (id: string): ItemMode =>
    editing?.id === id ? editing.mode : "view";

  let list = (
    <p className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">
      No conversations yet
    </p>
  );
  if (state.phase === "loading") {
    list = (
      <p className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">Loading…</p>
    );
  } else if (state.conversations.length > 0) {
    list = (
      <ul aria-label="Conversations" className="mt-2 flex flex-col gap-0.5">
        {state.conversations.map((conversation) => (
          <SidebarItem
            key={conversation.id}
            conversation={conversation}
            selected={conversation.id === state.selectedId}
            disabled={disabled}
            mode={modeOf(conversation.id)}
            onSelect={() => {
              onSelect(conversation.id);
            }}
            onMode={(mode) => {
              setEditing(
                mode === "view" ? null : { id: conversation.id, mode }
              );
            }}
            onRename={(title) => {
              void onRename(conversation.id, title).then((saved) => {
                if (saved) {
                  setEditing(null);
                }
              });
            }}
            onDelete={() => {
              setEditing(null);
              onDelete(conversation.id);
            }}
          />
        ))}
      </ul>
    );
  }

  return (
    <aside className="flex w-64 flex-col border-r border-zinc-200 p-4 dark:border-zinc-800">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold">Conversations</h2>
        <button
          type="button"
          onClick={onNew}
          disabled={disabled}
          className="rounded border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          New chat
        </button>
      </div>
      {state.error !== null && (
        <p role="alert" className="mt-2 text-xs text-red-600 dark:text-red-400">
          {state.error.message}
        </p>
      )}
      <nav className="min-h-0 flex-1 overflow-y-auto">{list}</nav>
      <p className="text-xs text-zinc-400 dark:text-zinc-500">
        {version === "" ? "" : `v${version}`}
      </p>
    </aside>
  );
};

export { Sidebar, SidebarItem };
