import { Ghost, Pencil, SquarePen, Trash } from "lucide-react";
import type { SubmitEvent } from "react";
import { useState } from "react";

import type { AppError, Conversation, Project } from "~/lib/types";

import { MemoryPanel } from "./memory-panel";
import { MemoryPreview } from "./memory-preview";

const button =
  "flex items-center gap-1 rounded border border-zinc-300 px-2 py-1 text-sm hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";

/** What deleting asks: saved chats are counted, temporary ones closed. */
const deleteQuestion = (name: string, chats: number): string =>
  `Delete “${name}” and its ${chats} saved ${chats === 1 ? "chat" : "chats"}? An open temporary chat in it is closed too.`;

type ProjectViewProps = {
  project: Project;
  /** The project's saved chats, most recently used first. */
  chats: Conversation[];
  /** True while a saved chat's reply streams: changes wait for it. */
  disabled: boolean;
  /** False while any reply runs: a temporary chat starts only from idle. */
  canStartTemporary: boolean;
  error: AppError | null;
  /** Resolves `false` when the name was refused, keeping the editor open. */
  onRename: (name: string) => Promise<boolean>;
  onInstructions: (instructions: string) => void;
  onAutoMemory: (on: boolean) => void;
  onNewChat: () => void;
  onNewTemporary: () => void;
  onOpenChat: (id: string) => void;
  onDelete: () => void;
};

/** A project's name, instructions, memory and chats. */
const ProjectView = ({
  project,
  chats,
  disabled,
  canStartTemporary,
  error,
  onRename,
  onInstructions,
  onAutoMemory,
  onNewChat,
  onNewTemporary,
  onOpenChat,
  onDelete,
}: ProjectViewProps) => {
  const [mode, setMode] = useState<"view" | "rename" | "confirm">("view");
  const [previewing, setPreviewing] = useState(false);
  const titleOf = (id: string): string =>
    chats.find((chat) => chat.id === id)?.title ?? "A chat";

  const onSubmitName = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = new FormData(event.currentTarget).get("name");
    if (typeof name === "string" && name.trim() !== "") {
      void onRename(name).then((saved) => {
        if (saved) {
          setMode("view");
        }
      });
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-4">
        {mode === "rename" ? (
          <form onSubmit={onSubmitName} className="flex items-center gap-2">
            <label className="sr-only" htmlFor="project-name">
              Project name
            </label>
            <input
              id="project-name"
              name="name"
              defaultValue={project.name}
              maxLength={200}
              ref={(element) => {
                element?.focus();
              }}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  setMode("view");
                }
              }}
              className="min-w-0 flex-1 rounded border border-zinc-300 bg-white px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
            />
            <button type="submit" className={button}>
              Save
            </button>
            <button
              type="button"
              onClick={() => {
                setMode("view");
              }}
              className={button}
            >
              Cancel
            </button>
          </form>
        ) : (
          <div className="flex items-center gap-2">
            <h2 className="min-w-0 flex-1 truncate text-2xl font-semibold">
              {project.name}
            </h2>
            <button
              type="button"
              onClick={() => {
                setMode("rename");
              }}
              aria-label={`Rename ${project.name}`}
              title="Rename"
              className={button}
            >
              <Pencil aria-hidden="true" className="size-4" />
            </button>
            <button
              type="button"
              onClick={() => {
                setMode("confirm");
              }}
              disabled={disabled}
              aria-label={`Delete ${project.name}`}
              title="Delete project"
              className={button}
            >
              <Trash aria-hidden="true" className="size-4" />
            </button>
          </div>
        )}
        {mode === "confirm" && (
          <div className="rounded bg-red-50 px-3 py-2 text-sm dark:bg-red-950">
            <p>{deleteQuestion(project.name, chats.length)}</p>
            <div className="mt-2 flex gap-2">
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
                  setMode("view");
                }}
                className={button}
              >
                Keep
              </button>
            </div>
          </div>
        )}
        {error !== null && (
          <p role="alert" className="text-sm text-red-600 dark:text-red-400">
            {error.message}
          </p>
        )}
        <div className="flex flex-col gap-1">
          <label htmlFor="project-instructions" className="font-medium">
            Instructions
          </label>
          {/* Saved when it loses focus; remounted when the saved text changes. */}
          <textarea
            key={`${project.id}:${project.instructions}`}
            id="project-instructions"
            defaultValue={project.instructions}
            rows={6}
            placeholder="How the model should answer in every chat of this project"
            onBlur={(event) => {
              const instructions = event.currentTarget.value;
              if (instructions !== project.instructions) {
                onInstructions(instructions);
              }
            }}
            className="resize-y rounded border border-zinc-300 bg-white px-2 py-1 text-sm dark:border-zinc-700 dark:bg-zinc-900"
          />
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            Sent ahead of each chat&apos;s own system prompt, from its next
            message.
          </p>
        </div>
        <MemoryPanel
          projectId={project.id}
          autoMemory={project.autoMemory}
          disabled={disabled}
          titleOf={titleOf}
          onAutoMemory={onAutoMemory}
          onPreview={() => {
            setPreviewing(true);
          }}
        />
        {previewing && (
          <MemoryPreview
            projectId={project.id}
            conversationId={null}
            onClose={() => {
              setPreviewing(false);
            }}
          />
        )}
        <div className="flex gap-2">
          <button
            type="button"
            onClick={onNewChat}
            disabled={disabled}
            className={button}
          >
            <SquarePen aria-hidden="true" className="size-4" />
            New chat
          </button>
          <button
            type="button"
            onClick={onNewTemporary}
            disabled={disabled || !canStartTemporary}
            className={button}
          >
            <Ghost aria-hidden="true" className="size-4" />
            New temporary chat
          </button>
        </div>
        {chats.length === 0 ? (
          <p className="text-sm text-zinc-500 dark:text-zinc-400">
            No chats in this project yet
          </p>
        ) : (
          <ul aria-label="Project chats" className="flex flex-col gap-0.5">
            {chats.map((chat) => (
              <li key={chat.id}>
                <button
                  type="button"
                  onClick={() => {
                    onOpenChat(chat.id);
                  }}
                  disabled={disabled}
                  className="w-full truncate rounded px-2 py-1 text-left text-sm hover:bg-zinc-100 disabled:cursor-not-allowed dark:hover:bg-zinc-900"
                >
                  {chat.title}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
};

export { deleteQuestion, ProjectView };
