import { Ghost } from "lucide-react";
import type { Dispatch, SetStateAction, UIEvent } from "react";
import { useEffect, useRef, useState } from "react";

import { stopsReply } from "~/features/chat/chat-keys";
import type { ChatState } from "~/features/chat/chat-reducer";
import { ChatSettings } from "~/features/chat/chat-settings";
import { CompareView } from "~/features/chat/compare-view";
import { Composer } from "~/features/chat/composer";
import { MessageList } from "~/features/chat/message-list";
import { pickModel } from "~/features/chat/pick-model";
import { toAppError } from "~/lib/app-error";
import { ollamaListModels, ollamaModelCapabilities } from "~/lib/ipc";
import type {
  AppError,
  ConversationSettings,
  ModelCapabilities,
  ModelInfo,
} from "~/lib/types";

const NOTICE_TEXT: Record<NonNullable<ChatState["notice"]>, string> = {
  busy: "Still answering. Stop it or wait for it to finish.",
  length: "The reply reached the length limit and was cut off.",
  compare: "Keep one of the compared answers or discard them first.",
};

type Models =
  | { phase: "idle" }
  | { phase: "ready"; models: ModelInfo[] }
  | { phase: "failed"; error: AppError };

/** Capabilities are only known for models looked up so far. */
const modelLabel = (
  model: ModelInfo,
  caps: ModelCapabilities | undefined
): string => {
  const name =
    model.parameterSize === null
      ? model.name
      : `${model.name} (${model.parameterSize})`;
  const tags = [
    caps?.thinking === true ? " · thinking" : "",
    caps?.vision === true ? " · vision" : "",
  ];
  return name + tags.join("");
};

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
}: ChatViewProps) => {
  const [models, setModels] = useState<Models>({ phase: "idle" });
  const [modelsAttempt, setModelsAttempt] = useState(0);
  // Looked up lazily, one audited `/api/show` per model.
  const [capabilities, setCapabilities] = useState<
    Record<string, ModelCapabilities>
  >({});
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  // Follow new tokens unless the user has scrolled up to read.
  const followBottom = useRef(true);
  const empty =
    state.messages.length === 0 && state.compare === null && !state.loading;

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
    const element = event.currentTarget;
    followBottom.current =
      element.scrollHeight - element.scrollTop - element.clientHeight < 32;
  };

  // (Re)load the model list whenever Ollama becomes reachable, or on retry.
  useEffect(() => {
    let cancelled = false;
    if (reachable) {
      void ollamaListModels().then(
        (list) => {
          if (!cancelled) {
            setModels({ phase: "ready", models: list });
            // The base URL may have changed: another Ollama, other models.
            setCapabilities({});
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
  const selectedCaps: ModelCapabilities | undefined = capabilities[selected];
  const known = selectedCaps !== undefined;

  useEffect(() => {
    let cancelled = false;
    if (reachable && selected !== "" && !known) {
      // A failed lookup just shows no Think control; Rust checks again on
      // send.
      void ollamaModelCapabilities(selected).then(
        (caps) => {
          if (!cancelled) {
            setCapabilities((current) => ({ ...current, [selected]: caps }));
          }
        },
        () => undefined
      );
    }
    return () => {
      cancelled = true;
    };
  }, [reachable, selected, known]);

  // Esc stops the reply wherever focus is, not only in the composer.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        stopsReply(event, {
          streaming,
          inTextField: event.target instanceof HTMLInputElement,
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
        <label htmlFor="model" className="text-sm font-medium">
          Model
        </label>
        <select
          id="model"
          value={selected}
          onChange={(event) => {
            setModel(event.target.value);
          }}
          disabled={modelList.length === 0}
          className="rounded border border-zinc-300 bg-white px-2 py-1 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        >
          {modelList.length === 0 && <option value="">No models</option>}
          {modelList.map((info) => (
            <option key={info.name} value={info.name}>
              {modelLabel(info, capabilities[info.name])}
            </option>
          ))}
        </select>
        <ChatSettings
          settings={state.settings}
          onChange={onSettings}
          caps={selectedCaps}
          disabled={state.loading}
        />
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
          focus. */}
      {empty ? (
        <div className="flex flex-1 flex-col items-center justify-end gap-2 pb-4 text-center">
          <h2 className="text-2xl font-semibold">How can I help?</h2>
          {selected === "" && (
            <p className="text-sm text-zinc-500 dark:text-zinc-400">
              Pick a model and send a message to start.
            </p>
          )}
        </div>
      ) : (
        <div
          ref={scroller}
          onScroll={onScroll}
          role="region"
          aria-label="Conversation"
          className="flex min-h-0 flex-1 flex-col overflow-y-auto rounded focus-visible:outline-2 focus-visible:outline-zinc-400"
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
      <div className={`mx-auto w-full max-w-3xl ${empty ? "flex-1" : ""}`}>
        <Composer
          streaming={streaming}
          canSend={selected !== "" && !state.loading && comparison === null}
          onSend={(text) => send(selected, text)}
          onStop={stop}
          temporary={state.temporary}
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
