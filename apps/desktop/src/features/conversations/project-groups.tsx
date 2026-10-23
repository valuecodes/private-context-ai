import { ChevronDown, ChevronRight, Folder, FolderPlus } from "lucide-react";
import type { ReactNode, SubmitEvent } from "react";
import { useState } from "react";

import type { Conversation, Project } from "~/lib/types";

const smallButton =
  "rounded px-1.5 py-0.5 text-xs text-zinc-500 hover:bg-zinc-200 disabled:opacity-40 dark:text-zinc-400 dark:hover:bg-zinc-800";

const iconButton =
  "rounded p-1 text-zinc-500 hover:bg-zinc-200 disabled:opacity-40 dark:text-zinc-400 dark:hover:bg-zinc-800";

type NewProjectProps = {
  disabled: boolean;
  /** Resolves `false` when the name was refused, keeping the form open. */
  onCreate: (name: string) => Promise<boolean>;
};

/** The "New project" button, which turns into a name form. */
const NewProject = ({ disabled, onCreate }: NewProjectProps) => {
  const [open, setOpen] = useState(false);
  if (!open) {
    return (
      <button
        type="button"
        onClick={() => {
          setOpen(true);
        }}
        disabled={disabled}
        aria-label="New project"
        title="New project"
        className={iconButton}
      >
        <FolderPlus aria-hidden="true" className="size-4" />
      </button>
    );
  }
  const onSubmit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = new FormData(event.currentTarget).get("name");
    if (typeof name === "string" && name.trim() !== "") {
      void onCreate(name).then((created) => {
        if (created) {
          setOpen(false);
        }
      });
    }
  };
  return (
    <form onSubmit={onSubmit} className="mt-1 flex items-center gap-1">
      <label className="sr-only" htmlFor="new-project-name">
        Project name
      </label>
      <input
        id="new-project-name"
        name="name"
        maxLength={200}
        placeholder="Project name"
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            setOpen(false);
          }
        }}
        ref={(element) => {
          element?.focus();
        }}
        className="min-w-0 flex-1 rounded border border-zinc-300 bg-white px-1.5 py-0.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
      />
      <button type="submit" className={smallButton}>
        Create
      </button>
      <button
        type="button"
        onClick={() => {
          setOpen(false);
        }}
        className={smallButton}
      >
        Cancel
      </button>
    </form>
  );
};

type ProjectGroupsProps = {
  /** Sorted by name. */
  projects: Project[];
  conversations: Conversation[];
  /** The open saved chat: its group is always expanded. */
  selectedId: string | null;
  /** The project whose view is open, if any. */
  selectedProjectId: string | null;
  disabled: boolean;
  onOpenProject: (id: string) => void;
  /** One chat's sidebar item. */
  renderChat: (conversation: Conversation) => ReactNode;
};

/** Each project, opening its view, with a collapsible list of its chats. */
const ProjectGroups = ({
  projects,
  conversations,
  selectedId,
  selectedProjectId,
  disabled,
  onOpenProject,
  renderChat,
}: ProjectGroupsProps) => {
  // Projects whose chats are listed.
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const openChatProject = conversations.find(
    ({ id }) => id === selectedId
  )?.projectId;

  const toggle = (id: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(id)) {
        next.add(id);
      }
      return next;
    });
  };

  return (
    <ul aria-label="Projects" className="mt-1 flex flex-col gap-0.5">
      {projects.map((project) => {
        const open = expanded.has(project.id) || openChatProject === project.id;
        const chats = conversations.filter(
          ({ projectId }) => projectId === project.id
        );
        const current = project.id === selectedProjectId;
        return (
          <li key={project.id}>
            <div
              className={`flex items-center rounded ${current ? "bg-zinc-200 dark:bg-zinc-800" : "hover:bg-zinc-100 dark:hover:bg-zinc-900"}`}
            >
              <button
                type="button"
                onClick={() => {
                  toggle(project.id);
                }}
                aria-expanded={open}
                aria-label={`Chats in ${project.name}`}
                className={iconButton}
              >
                {open ? (
                  <ChevronDown aria-hidden="true" className="size-3.5" />
                ) : (
                  <ChevronRight aria-hidden="true" className="size-3.5" />
                )}
              </button>
              <button
                type="button"
                onClick={() => {
                  onOpenProject(project.id);
                }}
                disabled={disabled}
                aria-current={current ? "true" : undefined}
                className="flex min-w-0 flex-1 items-center gap-1.5 truncate px-1 py-1 text-left text-sm disabled:cursor-not-allowed"
              >
                <Folder aria-hidden="true" className="size-3.5 shrink-0" />
                <span className="truncate">{project.name}</span>
              </button>
            </div>
            {open && (
              <ul
                aria-label={`${project.name} chats`}
                className="ml-4 flex flex-col gap-0.5"
              >
                {chats.length === 0 ? (
                  <li className="px-2 py-1 text-xs text-zinc-500 dark:text-zinc-400">
                    No chats yet
                  </li>
                ) : (
                  chats.map(renderChat)
                )}
              </ul>
            )}
          </li>
        );
      })}
    </ul>
  );
};

export { NewProject, ProjectGroups };
