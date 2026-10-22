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
import { SettingsView } from "~/features/settings/settings-view";
import { OllamaStatus } from "~/features/status/ollama-status";
import { isReachable } from "~/features/status/status-state";
import { useOllamaStatus } from "~/features/status/use-ollama-status";
import { StorageScreen } from "~/features/storage/storage-screen";
import { toAppError } from "~/lib/app-error";
import { settingsGet, storageStatus } from "~/lib/ipc";
import type { StorageStatus } from "~/lib/types";

type View = "chat" | "activity" | "settings";

const VIEWS: { id: View; label: string; Icon: LucideIcon }[] = [
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
  const { status, retry } = useOllamaStatus();
  const conversations = useConversations();
  const chat = useChatStream({
    onCreated: conversations.created,
    onStarted: conversations.touched,
  });
  // A temporary chat can be left while it answers; leaving discards it.
  const locked = chat.state.phase === "streaming" && !chat.state.temporary;

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

  const open = (id: string | null) => {
    if (locked) {
      return;
    }
    setView("chat");
    conversations.select(id);
    chat.open(id);
    const saved = conversations.state.conversations.find(
      (conversation) => conversation.id === id
    );
    if (saved !== undefined && saved.model !== "") {
      setModel(saved.model);
    }
  };

  const temporary = (on: boolean) => {
    // Never selected in the sidebar: a temporary chat is not listed there.
    conversations.select(null);
    if (on) {
      chat.openTemporary();
    } else {
      chat.open(null);
    }
  };

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
        disabled={locked}
        version={version}
        onNew={() => {
          open(null);
        }}
        onSelect={open}
        onRename={conversations.rename}
        onDelete={remove}
        onCopy={conversations.copyMarkdown}
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
                setView(id);
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
              stop={chat.stop}
              onSettings={chat.updateSettings}
              onTemporary={temporary}
              model={model}
              setModel={setModel}
              defaultModel={defaultModel}
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
