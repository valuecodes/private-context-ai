import { ArrowDown, Ghost } from "lucide-react";
import type { Dispatch, SetStateAction, UIEvent } from "react";
import { useEffect, useRef, useState } from "react";

import { stopsReply } from "~/features/chat/chat-keys";
import type { ChatState } from "~/features/chat/chat-reducer";
import { ChatSettings } from "~/features/chat/chat-settings";
import type { ChatSettingsProps } from "~/features/chat/chat-settings";
import { CompareView } from "~/features/chat/compare-view";
import { Composer } from "~/features/chat/composer";
import { ComposerControls } from "~/features/chat/composer-controls";
import { MessageList } from "~/features/chat/message-list";
import { pickModel } from "~/features/chat/pick-model";
import { followsAfterScroll, isAtBottom } from "~/features/chat/scroll";
import { useModelCapabilities } from "~/features/chat/use-model-capabilities";
import { MemoryIndicator } from "~/features/projects/memory-indicator";
import type { ProjectMemoryStatus } from "~/features/projects/use-memory-status";
import { toAppError } from "~/lib/app-error";
import { ollamaListModels } from "~/lib/ipc";
import type { AppError, ConversationSettings, ModelInfo } from "~/lib/types";

const NOTICE_TEXT: Record<NonNullable<ChatState["notice"]>, string> = {
  busy: "Still answering. Stop it or wait for it to finish.",
  length: "The reply reached the length limit and was cut off.",
  compare: "Keep one of the compared answers or discard them first.",
};

type Models =
  | { phase: "idle" }
  | { phase: "ready"; models: ModelInfo[] }
  | { phase: "failed"; error: AppError };

type ChatViewProps = {
  reachable: boolean;
  state: ChatState;
  send: (model: string, text: string) => boolean;
  regenerate: (model: string) => boolean;
  edit: (model: string, messageId: string, text: string) => boolean;
  /** Asks `models` the prompt `text` one after another; saved chats only. */
  compare: (models: string[], text: string) => boolean;
  /** Keeps a compared answer; resolves `false` when refused. */
  pick: (messageId: string) => Promise<boolean>;
  discard: () => void;
  stop: () => void;
  /** The open chat's settings changed; applied from its next send. */
  onSettings: (settings: ConversationSettings) => void;
  /** Turns an empty new chat into a temporary one, or back. */
  onTemporary: (on: boolean) => void;
  /** Owned by the parent, so opening a conversation can pick its model. */
  model: string;
  setModel: Dispatch<SetStateAction<string>>;
  /** From Settings; used when `model` is empty or not installed. */
  defaultModel: string;
  /** The open chat's project, if it is in one. */
  project?: ChatSettingsProps["project"];
  /** The open saved chat's background memory, shown in the header. */
  memory?: {
    state: ProjectMemoryStatus["state"];
    onOpenActivity: () => void;
  };
};

const ChatView = ({
  reachable,
  state,
  send,
  regenerate,
  edit,
  compare,
  pick,
  discard,
  stop,
  onSettings,
  onTemporary,
  model,
  setModel,
  defaultModel,
  project,
  memory,
}: ChatViewProps) => {
  const [models, setModels] = useState<Models>({ phase: "idle" });
  const [modelsAttempt, setModelsAttempt] = useState(0);
  // Bumped on every list load: the base URL may have changed, so another
  // Ollama with other models.
  const [listVersion, setListVersion] = useState(0);
  // Looked up lazily, one audited `/api/show` per model.
  const { capabilities, lookUp, loadAll } = useModelCapabilities({
    reachable,
    listVersion,
  });
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  // Follow new tokens unless the user has scrolled up to read.
  const followBottom = useRef(true);
  // Whether the view is at the end, which hides the scroll-down button. Not
  // the same as following: a jump down follows before it arrives.
  const [atBottom, setAtBottom] = useState(true);
  // A jump to the newest message is under way.
  const jumping = useRef(false);
  const lastScrollTop = useRef(0);
  const empty =
    state.messages.length === 0 && state.compare === null && !state.loading;

  // Another chat starts at its end. `empty` too: a new chat unmounts the
  // scroller, and the next one mounts at the top.
  const chatKey = `${String(state.temporary)}:${state.conversationId ?? ""}:${String(empty)}`;
  const [shownChat, setShownChat] = useState(chatKey);
  if (shownChat !== chatKey) {
    setShownChat(chatKey);
    setAtBottom(true);
  }
  useEffect(() => {
    followBottom.current = true;
    jumping.current = false;
    lastScrollTop.current = 0;
  }, [chatKey]);

  // Follows the content's height rather than `state.messages`: a reply's
  // Markdown renders deferred, after the message state has changed.
  useEffect(() => {
    const element = scroller.current;
    const inner = content.current;
    if (empty || element === null || inner === null) {
      return undefined;
    }
    const observer = new ResizeObserver(() => {
      if (followBottom.current) {
        element.scrollTop = element.scrollHeight;
      }
    });
    observer.observe(inner);
    return () => {
      observer.disconnect();
    };
  }, [empty]);

  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const { scrollHeight, scrollTop, clientHeight } = event.currentTarget;
    const at = isAtBottom(scrollHeight, scrollTop, clientHeight);
    followBottom.current = followsAfterScroll({
      atBottom: at,
      jumping: jumping.current,
      scrollTop,
      lastScrollTop: lastScrollTop.current,
    });
    if (at || !followBottom.current) {
      jumping.current = false;
    }
    lastScrollTop.current = scrollTop;
    setAtBottom(at);
  };

  // The user's own wheel, touch, pointer or keys take over from a jump.
  useEffect(() => {
    const element = scroller.current;
    if (empty || element === null) {
      return undefined;
    }
    const endJump = () => {
      jumping.current = false;
    };
    const events = ["wheel", "touchstart", "pointerdown", "keydown"] as const;
    for (const name of events) {
      element.addEventListener(name, endJump, { passive: true });
    }
    return () => {
      for (const name of events) {
        element.removeEventListener(name, endJump);
      }
    };
  }, [empty]);

  const jumpToBottom = () => {
    const element = scroller.current;
    if (element === null) {
      return;
    }
    followBottom.current = true;
    jumping.current = true;
    const still = globalThis.matchMedia(
      "(prefers-reduced-motion: reduce)"
    ).matches;
    element.scrollTo({
      top: element.scrollHeight,
      behavior: still ? "auto" : "smooth",
    });
  };

  // (Re)load the model list whenever Ollama becomes reachable, or on retry.
  useEffect(() => {
    let cancelled = false;
    if (reachable) {
      void ollamaListModels().then(
        (list) => {
          if (!cancelled) {
            setModels({ phase: "ready", models: list });
            setListVersion((version) => version + 1);
          }
        },
        (error: unknown) => {
          if (!cancelled) {
            setModels({ phase: "failed", error: toAppError(error) });
          }
        }
      );
    }
    return () => {
      cancelled = true;
    };
  }, [reachable, modelsAttempt]);

  // Embedding-only models cannot chat; they drop out once looked up.
  const modelList = (models.phase === "ready" ? models.models : []).filter(
    (info) => capabilities[info.name]?.embeddingOnly !== true
  );
  // An opened conversation may name a model that is no longer installed.
  const selected = pickModel(modelList, model, defaultModel);
  // A reply or a comparison is running: Stop applies to either.
  const streaming = state.phase !== "idle";
  const comparison = state.compare;
  // Only a saved chat compares: the candidates are saved rows.
  const canCompare = !state.temporary && state.conversationId !== null;
  // Only an empty new chat can switch: a started chat is saved or not.
  const canToggleTemporary =
    state.conversationId === null &&
    state.messages.length === 0 &&
    !streaming &&
    !state.loading;
  // A failed lookup just shows no Think control; Rust checks again on send.
  // `listVersion` looks the selected model up again after a reload.
  useEffect(() => {
    lookUp(selected);
  }, [selected, listVersion, lookUp]);

  // Esc stops the reply wherever focus is, not only in the composer.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        stopsReply(event, {
          streaming,
          inTextField: event.target instanceof HTMLInputElement,
          // An open menu closed on this Esc instead.
          handled: event.defaultPrevented,
        })
      ) {
        event.preventDefault();
        stop();
      }
    };
    globalThis.addEventListener("keydown", onKeyDown);
    return () => {
      globalThis.removeEventListener("keydown", onKeyDown);
    };
  }, [streaming, stop]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 p-4">
      <div className="flex items-center gap-2">
        <ChatSettings
          settings={state.settings}
          onChange={onSettings}
          disabled={state.loading}
          project={project}
        />
        {memory !== undefined && (
          <MemoryIndicator
            state={memory.state}
            onOpenActivity={memory.onOpenActivity}
          />
        )}
        {models.phase === "failed" && (
          <>
            <span className="text-xs text-red-600 dark:text-red-400">
              Could not list models: {models.error.message}
            </span>
            <button
              type="button"
              onClick={() => {
                setModelsAttempt((count) => count + 1);
              }}
              className="rounded border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
            >
              Retry
            </button>
          </>
        )}
        <button
          type="button"
          aria-pressed={state.temporary}
          disabled={!canToggleTemporary}
          title="A temporary chat is never saved and is cleared when you leave"
          onClick={() => {
            onTemporary(!state.temporary);
          }}
          className={`ml-auto flex items-center gap-1 rounded border px-2 py-0.5 text-sm disabled:cursor-not-allowed disabled:opacity-40 ${state.temporary ? "border-zinc-900 bg-zinc-900 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900" : "border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"}`}
        >
          <Ghost aria-hidden="true" className="size-4" />
          Temporary
        </button>
      </div>
      {state.temporary && (
        <p className="flex items-center justify-center gap-2 rounded border border-dashed border-zinc-300 px-3 py-1.5 text-sm text-zinc-600 dark:border-zinc-700 dark:text-zinc-300">
          <Ghost aria-hidden="true" className="size-4" />
          Temporary chat — not saved, cleared when you leave
        </p>
      )}
      {/* Only this slot switches between the empty state and the messages,
          so the composer below stays mounted and keeps its focus on the
          first send. WebView2 makes an overflowing scroller
          keyboard-focusable on its own; the label names it when it gets
          focus. `relative` makes it the containing block of the messages'
          sr-only text, which would otherwise sit below the scrolled-off part
          and stretch the page. */}
      {empty ? (
        <div className="flex flex-1 flex-col items-center justify-end gap-2 pb-4 text-center">
          <h2 className="text-2xl font-semibold">How can I help?</h2>
          {project !== undefined && (
            <p className="text-sm text-zinc-500 dark:text-zinc-400">
              {`In project ${project.name}`}
            </p>
          )}
        </div>
      ) : (
        <div
          ref={scroller}
          onScroll={onScroll}
          role="region"
          aria-label="Conversation"
          className="relative flex min-h-0 flex-1 flex-col overflow-y-auto rounded focus-visible:outline-2 focus-visible:outline-zinc-400"
        >
          <div ref={content} className="mx-auto w-full max-w-3xl">
            <MessageList
              messages={state.messages}
              busy={
                streaming ||
                state.loading ||
                selected === "" ||
                comparison !== null
              }
              onRegenerate={() => {
                regenerate(selected);
              }}
              onEdit={(messageId, text) => edit(selected, messageId, text)}
            />
            {comparison !== null && (
              <div className={state.messages.length === 0 ? "" : "mt-4"}>
                <CompareView
                  compare={comparison}
                  comparing={state.phase === "comparing"}
                  onPick={(messageId) => {
                    const model = comparison.candidates.find(
                      ({ id }) => id === messageId
                    )?.model;
                    void pick(messageId).then((kept) => {
                      if (kept && model !== undefined) {
                        setModel(model);
                      }
                    });
                  }}
                  onDiscard={discard}
                />
              </div>
            )}
          </div>
        </div>
      )}
      {state.notice !== null && (
        <p role="status" className="text-sm text-amber-700 dark:text-amber-400">
          {NOTICE_TEXT[state.notice]}
        </p>
      )}
      {state.error !== null && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {state.error.message}
        </p>
      )}
      {/* `relative`: the scroll-down button sits just above the composer,
          outside the scroller so scrolling never moves it. */}
      <div
        className={`relative mx-auto w-full max-w-3xl ${empty ? "flex-1" : ""}`}
      >
        {!empty && !atBottom && (
          <button
            type="button"
            onClick={jumpToBottom}
            aria-label="Scroll to newest message"
            title="Scroll to newest message"
            className="absolute -top-12 left-1/2 z-10 -translate-x-1/2 rounded-full border border-zinc-300 bg-white p-2 text-zinc-600 shadow-sm hover:bg-zinc-100 focus-visible:outline-2 focus-visible:outline-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300 dark:hover:bg-zinc-800"
          >
            <ArrowDown aria-hidden="true" className="size-4" />
          </button>
        )}
        <Composer
          streaming={streaming}
          canSend={selected !== "" && !state.loading && comparison === null}
          onSend={(text) => send(selected, text)}
          onStop={stop}
          temporary={state.temporary}
          toolbar={
            <ComposerControls
              models={modelList}
              capabilities={capabilities}
              selected={selected}
              onSelect={setModel}
              onOpenPicker={() => {
                loadAll(modelList.map(({ name }) => name));
              }}
              settings={state.settings}
              onSettings={onSettings}
              disabled={state.loading}
            />
          }
          compare={
            canCompare
              ? {
                  models: modelList.map(({ name }) => name),
                  current: selected,
                  onCompare: (text, models) => compare(models, text),
                }
              : undefined
          }
        />
      </div>
    </div>
  );
};

export { ChatView };
