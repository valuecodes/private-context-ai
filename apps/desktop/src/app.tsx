import { useEffect, useState } from "react";

import { loadVersion } from "~/app-version";
import { ChatView } from "~/features/chat/chat-view";
import { OllamaStatus } from "~/features/status/ollama-status";
import { isReachable } from "~/features/status/status-state";
import { useOllamaStatus } from "~/features/status/use-ollama-status";

const App = () => {
  const [version, setVersion] = useState("");
  const { status, retry } = useOllamaStatus();

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

  return (
    <div className="flex h-screen bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      <aside className="flex w-64 flex-col border-r border-zinc-200 p-4 dark:border-zinc-800">
        <h2 className="text-sm font-semibold">Conversations</h2>
        <p className="mt-2 flex-1 text-sm text-zinc-500 dark:text-zinc-400">
          No conversations yet
        </p>
        <p className="text-xs text-zinc-400 dark:text-zinc-500">
          {version === "" ? "" : `v${version}`}
        </p>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <main className="flex min-h-0 flex-1 flex-col">
          <h1 className="sr-only">Private Context AI</h1>
          <ChatView reachable={isReachable(status)} />
        </main>
        <footer className="border-t border-zinc-200 px-4 py-2 dark:border-zinc-800">
          <OllamaStatus status={status} onRetry={retry} />
        </footer>
      </div>
    </div>
  );
};

export { App };
