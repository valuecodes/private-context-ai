import { Check, Copy, Pencil, Search, SquarePen, Trash } from "lucide-react";
import type { KeyboardEvent, SubmitEvent } from "react";
import { useState } from "react";

import {
  filterConversations,
  useConversationSearch,
} from "~/features/conversations/use-conversation-search";
import type { ConversationsState } from "~/features/conversations/use-conversations";
import type { Conversation } from "~/lib/types";

const COPIED_MS = 1500;

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
  /** Fetches what a copy would put on the clipboard, ahead of the click. */
  onPrepareCopy: () => void;
  /** Resolves `false` when nothing was copied. */
  onCopy: () => Promise<boolean>;
};

const smallButton =
  "rounded px-1.5 py-0.5 text-xs text-zinc-500 hover:bg-zinc-200 disabled:opacity-40 dark:text-zinc-400 dark:hover:bg-zinc-800";

const iconButton =
  "rounded p-1 text-zinc-500 hover:bg-zinc-200 disabled:opacity-40 dark:text-zinc-400 dark:hover:bg-zinc-800";

const SidebarItem = ({
  conversation,
  selected,
  disabled,
  mode,
  onSelect,
  onMode,
  onRename,
  onDelete,
  onPrepareCopy,
  onCopy,
}: SidebarItemProps) => {
  const [copied, setCopied] = useState(false);
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
        title="Rename"
        className={iconButton}
      >
        <Pencil aria-hidden="true" className="size-3.5" />
      </button>
      {/* Read-only, so it works while a reply streams. */}
      <button
        type="button"
        onPointerEnter={onPrepareCopy}
        onFocus={onPrepareCopy}
        onClick={() => {
          void onCopy().then((ok) => {
            if (ok) {
              setCopied(true);
              setTimeout(() => {
                setCopied(false);
              }, COPIED_MS);
            }
          });
        }}
        aria-label={
          copied ? "Copied" : `Copy ${conversation.title} as Markdown`
        }
        title="Copy as Markdown"
        className={iconButton}
      >
        {copied ? (
          <Check aria-hidden="true" className="size-3.5" />
        ) : (
          <Copy aria-hidden="true" className="size-3.5" />
        )}
      </button>
      <button
        type="button"
        onClick={() => {
          onMode("confirm");
        }}
        disabled={disabled}
        aria-label={`Delete ${conversation.title}`}
        title="Delete"
        className={iconButton}
      >
        <Trash aria-hidden="true" className="size-3.5" />
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
  /** Fetches a conversation's Markdown when its copy button is pointed at. */
  onPrepareCopy: (id: string) => void;
  /** Copies the conversation as Markdown; resolves `false` on failure. */
  onCopy: (id: string) => Promise<boolean>;
};

const Sidebar = ({
  state,
  disabled,
  version,
  onNew,
  onSelect,
  onRename,
  onDelete,
  onPrepareCopy,
  onCopy,
}: SidebarProps) => {
  // At most one conversation is being renamed or confirmed at a time.
  const [editing, setEditing] = useState<{
    id: string;
    mode: ItemMode;
  } | null>(null);
  const [query, setQuery] = useState("");
  // `disabled` is true exactly while a saved chat's reply streams.
  const matches = useConversationSearch(query, state.conversations, disabled);
  const shown = filterConversations(state.conversations, query, matches);

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
  } else if (state.conversations.length > 0 && shown.length === 0) {
    list = (
      <p className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">
        No matching conversations
      </p>
    );
  } else if (shown.length > 0) {
    list = (
      <ul aria-label="Conversations" className="mt-2 flex flex-col gap-0.5">
        {shown.map((conversation) => (
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
            onPrepareCopy={() => {
              onPrepareCopy(conversation.id);
            }}
            onCopy={() => onCopy(conversation.id)}
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
          aria-label="New chat"
          title="New chat"
          className={iconButton}
        >
          <SquarePen aria-hidden="true" className="size-4" />
        </button>
      </div>
      <div className="relative mt-2">
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-zinc-400"
        />
        <input
          type="search"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
          maxLength={200}
          aria-label="Search conversations"
          placeholder="Search"
          className="w-full rounded border border-zinc-300 bg-white py-1 pr-2 pl-7 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        />
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
