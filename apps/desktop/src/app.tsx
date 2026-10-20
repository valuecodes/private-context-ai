import { useEffect, useState } from "react";

import { loadVersion } from "~/app-version";
import { ChatView } from "~/features/chat/chat-view";
import { useChatStream } from "~/features/chat/use-chat-stream";
import { Sidebar } from "~/features/conversations/sidebar";
import { useConversations } from "~/features/conversations/use-conversations";
import { OllamaStatus } from "~/features/status/ollama-status";
import { isReachable } from "~/features/status/status-state";
import { useOllamaStatus } from "~/features/status/use-ollama-status";
import { StorageScreen } from "~/features/storage/storage-screen";
import { toAppError } from "~/lib/app-error";
import { storageStatus } from "~/lib/ipc";
import type { StorageStatus } from "~/lib/types";

/** The sidebar and chat, shown once storage is open. */
const Workspace = () => {
  const [version, setVersion] = useState("");
  const [model, setModel] = useState("");
  const { status, retry } = useOllamaStatus();
  const conversations = useConversations();
  const chat = useChatStream({
    onCreated: conversations.created,
    onStarted: conversations.touched,
  });
  const streaming = chat.state.phase === "streaming";

  useEffect(() => {
    let cancelled = false;
    void loadVersion().then((value) => {
      if (!cancelled) {
        setVersion(value);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const open = (id: string | null) => {
    if (streaming) {
      return;
    }
    conversations.select(id);
    chat.open(id);
    const saved = conversations.state.conversations.find(
      (conversation) => conversation.id === id
    );
    if (saved !== undefined && saved.model !== "") {
      setModel(saved.model);
    }
  };

  const remove = (id: string) => {
    void conversations.remove(id).then((deleted) => {
      if (deleted && id === chat.state.conversationId) {
        chat.open(null);
      }
    });
  };

  return (
    <div className="flex h-screen bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      <Sidebar
        state={conversations.state}
        disabled={streaming}
        version={version}
        onNew={() => {
          open(null);
        }}
        onSelect={open}
        onRename={conversations.rename}
        onDelete={remove}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <main className="flex min-h-0 flex-1 flex-col">
          <h1 className="sr-only">Private Context AI</h1>
          <ChatView
            reachable={isReachable(status)}
            state={chat.state}
            send={chat.send}
            stop={chat.stop}
            model={model}
            setModel={setModel}
          />
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
