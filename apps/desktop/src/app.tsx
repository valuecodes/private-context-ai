import { Activity, MessageSquare, Settings } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";

import { loadVersion } from "~/app-version";
import { ActivityPanel } from "~/features/activity/activity-panel";
import { replyAnnouncement } from "~/features/chat/chat-reducer";
import { ChatView } from "~/features/chat/chat-view";
import { useChatStream } from "~/features/chat/use-chat-stream";
import { Sidebar } from "~/features/conversations/sidebar";
import { useConversations } from "~/features/conversations/use-conversations";
import { MemoryPreview } from "~/features/projects/memory-preview";
import { ProjectView } from "~/features/projects/project-view";
import { useMemoryStatus } from "~/features/projects/use-memory-status";
import { useProjects } from "~/features/projects/use-projects";
import { SettingsView } from "~/features/settings/settings-view";
import { OllamaStatus } from "~/features/status/ollama-status";
import { isReachable } from "~/features/status/status-state";
import { useOllamaStatus } from "~/features/status/use-ollama-status";
import { StorageScreen } from "~/features/storage/storage-screen";
import { toAppError } from "~/lib/app-error";
import { settingsGet, storageStatus } from "~/lib/ipc";
import type { StorageStatus } from "~/lib/types";

/** `project`: a project's view, opened from the sidebar; not a tab. */
type View = "chat" | "activity" | "settings" | "project";

const VIEWS: {
  id: Exclude<View, "project">;
  label: string;
  Icon: LucideIcon;
}[] = [
  { id: "chat", label: "Chat", Icon: MessageSquare },
  { id: "activity", label: "Activity", Icon: Activity },
  { id: "settings", label: "Settings", Icon: Settings },
];

/** The sidebar and the chat, activity and settings views, shown once
 * storage is open. Chat state lives here, so switching views keeps it. */
const Workspace = () => {
  const [version, setVersion] = useState("");
  const [model, setModel] = useState("");
  const [defaultModel, setDefaultModel] = useState("");
  const [view, setView] = useState<View>("chat");
  // The project whose view is open, or was last.
  const [projectId, setProjectId] = useState<string | null>(null);
  // The saved chat whose "What this chat sees" dialog is open.
  const [previewing, setPreviewing] = useState<string | null>(null);
  const { status, retry } = useOllamaStatus();
  const conversations = useConversations();
  const projects = useProjects();
  const chat = useChatStream({
    onCreated: conversations.created,
    onStarted: conversations.touched,
  });
  // A temporary chat can be left while it answers; leaving discards it.
  const locked = chat.state.phase !== "idle" && !chat.state.temporary;
  const memory = useMemoryStatus();
  const { reload: reloadMemoryCounts } = memory;

  // Projects and chats added, deleted or switched change the counts.
  useEffect(() => {
    reloadMemoryCounts();
  }, [
    reloadMemoryCounts,
    projects.state.projects,
    conversations.state.conversations,
  ]);

  useEffect(() => {
    let cancelled = false;
    void loadVersion().then((value) => {
      if (!cancelled) {
        setVersion(value);
      }
    });
    // Without settings the chat simply falls back to the first model.
    void settingsGet().then(
      (settings) => {
        if (!cancelled) {
          setDefaultModel(settings.defaultModel);
        }
      },
      () => undefined
    );
    return () => {
      cancelled = true;
    };
  }, []);

  /**
   * A saved conversation, or a new chat outside any project for `null`,
   * once project instructions edited just before are saved: a failed save
   * keeps the project view, with its error.
   */
  const open = (id: string | null) => {
    if (locked) {
      return;
    }
    const saved = conversations.state.conversations.find(
      (conversation) => conversation.id === id
    );
    void projects.settled().then((ok) => {
      if (!ok) {
        return;
      }
      setView("chat");
      conversations.select(id);
      chat.open(id, saved?.projectId ?? null);
      if (saved !== undefined && saved.model !== "") {
        setModel(saved.model);
      }
    });
  };

  const temporary = (on: boolean) => {
    // Never selected in the sidebar: a temporary chat is not listed there.
    conversations.select(null);
    // An empty new chat stays in its project either way.
    const { projectId: project } = chat.state;
    if (on) {
      chat.openTemporary(project);
    } else {
      chat.open(null, project);
    }
  };

  /**
   * Switches to a tab. Leaving a project waits for the instructions saved as
   * it lost focus, so the next send uses them; a failed save keeps the
   * project view, with its error.
   */
  const leaveTo = (next: View) => {
    if (view !== "project") {
      setView(next);
      return;
    }
    void projects.settled().then((saved) => {
      if (saved) {
        setView(next);
      }
    });
  };

  const openProject = (id: string) => {
    if (locked) {
      return;
    }
    setProjectId(id);
    setView("project");
  };

  /**
   * A new chat in project `id`, once its instructions are saved: a failed
   * save keeps the project view, with its error.
   */
  const newInProject = (id: string, temporaryChat: boolean) => {
    void projects.settled().then((saved) => {
      if (!saved) {
        return;
      }
      setView("chat");
      conversations.select(null);
      if (temporaryChat) {
        chat.openTemporary(id);
      } else {
        chat.open(null, id);
      }
    });
  };

  const removeProject = (id: string) => {
    void projects.remove(id).then((deleted) => {
      if (!deleted) {
        return;
      }
      conversations.projectDeleted(id);
      // Rust discarded its chats; a temporary one is closed here too.
      chat.closeProject(id);
      setView("chat");
    });
  };

  const shownProject =
    projects.state.projects.find((project) => project.id === projectId) ?? null;
  const chatProject = projects.state.projects.find(
    (project) => project.id === chat.state.projectId
  );
  // "What this chat sees" exists for a saved chat in a project only.
  const previewChat =
    chat.state.temporary || chat.state.conversationId === null
      ? null
      : chat.state.conversationId;
  const openActivity = () => {
    leaveTo("activity");
  };
  const chatMemory =
    chatProject === undefined ? undefined : memory.statuses[chatProject.id];

  const remove = (id: string) => {
    void conversations.remove(id).then((deleted) => {
      if (deleted) {
        chat.forget(id);
      }
    });
  };

  return (
    <div className="flex h-screen bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      <Sidebar
        state={conversations.state}
        projects={projects.state.projects}
        selectedProjectId={view === "project" ? projectId : null}
        disabled={locked}
        version={version}
        onNew={() => {
          open(null);
        }}
        onNewProject={(name) =>
          projects.create(name).then((project) => {
            if (project !== null) {
              openProject(project.id);
            }
            return project !== null;
          })
        }
        onOpenProject={openProject}
        onSelect={open}
        onRename={conversations.rename}
        onDelete={remove}
        onPrepareCopy={conversations.prepareMarkdown}
        onCopy={conversations.copyMarkdown}
        memory={memory.statuses}
        onOpenActivity={openActivity}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <nav
          aria-label="Views"
          className="flex gap-1 border-b border-zinc-200 px-4 pt-2 dark:border-zinc-800"
        >
          {VIEWS.map(({ id, label, Icon }) => (
            <button
              key={id}
              type="button"
              aria-current={view === id ? "page" : undefined}
              onClick={() => {
                leaveTo(id);
              }}
              className={`flex items-center gap-1.5 rounded-t px-3 py-1 text-sm ${view === id ? "border-b-2 border-zinc-900 font-medium dark:border-zinc-100" : "text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100"}`}
            >
              <Icon aria-hidden="true" className="size-4" />
              {label}
            </button>
          ))}
        </nav>
        <main className="flex min-h-0 flex-1 flex-col">
          <h1 className="sr-only">Private Context AI</h1>
          {/* Outside the views, so a reply that ends while Activity or
              Settings is open is still read out. */}
          <p role="status" className="sr-only">
            {replyAnnouncement(chat.state)}
          </p>
          {view === "chat" && (
            <ChatView
              reachable={isReachable(status)}
              state={chat.state}
              send={chat.send}
              regenerate={chat.regenerate}
              edit={chat.edit}
              compare={chat.compare}
              pick={chat.pick}
              discard={chat.discard}
              stop={chat.stop}
              onSettings={chat.updateSettings}
              onTemporary={temporary}
              model={model}
              setModel={setModel}
              defaultModel={defaultModel}
              project={
                chatProject === undefined
                  ? undefined
                  : {
                      name: chatProject.name,
                      onOpen: () => {
                        openProject(chatProject.id);
                      },
                      onPreview:
                        previewChat === null
                          ? undefined
                          : () => {
                              setPreviewing(previewChat);
                            },
                    }
              }
              memory={
                previewChat === null
                  ? undefined
                  : {
                      state:
                        chatMemory?.conversationId === previewChat
                          ? chatMemory.state
                          : null,
                      onOpenActivity: openActivity,
                    }
              }
            />
          )}
          {view === "chat" &&
            chatProject !== undefined &&
            previewing !== null &&
            previewing === previewChat && (
              <MemoryPreview
                projectId={chatProject.id}
                conversationId={previewing}
                onClose={() => {
                  setPreviewing(null);
                }}
              />
            )}
          {view === "project" && shownProject !== null && (
            <ProjectView
              // A fresh view per project, so a pending delete never carries over.
              key={shownProject.id}
              project={shownProject}
              chats={conversations.state.conversations.filter(
                (conversation) => conversation.projectId === shownProject.id
              )}
              disabled={locked}
              canStartTemporary={chat.state.phase === "idle"}
              error={projects.state.error}
              onRename={(name) => projects.rename(shownProject.id, name)}
              onInstructions={(instructions) => {
                projects.saveInstructions(shownProject.id, instructions);
              }}
              onAutoMemory={(on) => {
                projects.setAutoMemory(shownProject.id, on);
              }}
              onNewChat={() => {
                newInProject(shownProject.id, false);
              }}
              onNewTemporary={() => {
                newInProject(shownProject.id, true);
              }}
              onOpenChat={open}
              onDelete={() => {
                removeProject(shownProject.id);
              }}
              memory={memory.statuses[shownProject.id]}
              onMemoryChanged={reloadMemoryCounts}
            />
          )}
          {view === "activity" && <ActivityPanel />}
          {view === "settings" && (
            <SettingsView
              onSaved={(settings) => {
                setDefaultModel(settings.defaultModel);
                // A new base URL shows in the status line and model list.
                retry();
              }}
            />
          )}
        </main>
        <footer className="border-t border-zinc-200 px-4 py-2 dark:border-zinc-800">
          <OllamaStatus status={status} onRetry={retry} />
        </footer>
      </div>
    </div>
  );
};

/** Gates the app on secure storage: no chat UI without the encrypted DB. */
const App = () => {
  const [storage, setStorage] = useState<StorageStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    void storageStatus().then(
      (status) => {
        if (!cancelled) {
          setStorage(status);
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          setStorage({ state: "io", detail: toAppError(error).message });
        }
      }
    );
    return () => {
      cancelled = true;
    };
  }, []);

  if (storage === null) {
    return (
      <main className="flex h-screen items-center justify-center bg-zinc-50 text-sm text-zinc-500 dark:bg-zinc-950 dark:text-zinc-400">
        Opening secure storage…
      </main>
    );
  }
  if (storage.state !== "ok") {
    return <StorageScreen status={storage} />;
  }
  return <Workspace />;
};

export { App, Workspace };
