import type { Dispatch, SetStateAction, UIEvent } from "react";
import { useEffect, useRef, useState } from "react";

import { Composer } from "~/features/chat/composer";
import { MessageList } from "~/features/chat/message-list";
import { pickModel } from "~/features/chat/pick-model";
import type { ChatState } from "~/features/chat/use-chat-stream";
import { toAppError } from "~/lib/app-error";
import { ollamaListModels } from "~/lib/ipc";
import type { AppError, ModelInfo } from "~/lib/types";

const NOTICE_TEXT: Record<NonNullable<ChatState["notice"]>, string> = {
  busy: "Still answering. Stop it or wait for it to finish.",
  length: "The reply reached the length limit and was cut off.",
};

type Models =
  | { phase: "idle" }
  | { phase: "ready"; models: ModelInfo[] }
  | { phase: "failed"; error: AppError };

const modelLabel = (model: ModelInfo): string =>
  model.parameterSize === null
    ? model.name
    : `${model.name} (${model.parameterSize})`;

type ChatViewProps = {
  reachable: boolean;
  state: ChatState;
  send: (model: string, text: string) => boolean;
  stop: () => void;
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
  stop,
  model,
  setModel,
  defaultModel,
}: ChatViewProps) => {
  const [models, setModels] = useState<Models>({ phase: "idle" });
  const [modelsAttempt, setModelsAttempt] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  // Follow new tokens unless the user has scrolled up to read.
  const followBottom = useRef(true);

  useEffect(() => {
    const element = scroller.current;
    if (element !== null && followBottom.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [state.messages]);

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

  const modelList = models.phase === "ready" ? models.models : [];
  // An opened conversation may name a model that is no longer installed.
  const selected = pickModel(modelList, model, defaultModel);
  const streaming = state.phase === "streaming";

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
              {modelLabel(info)}
            </option>
          ))}
        </select>
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
      </div>
      <div
        ref={scroller}
        onScroll={onScroll}
        className="flex min-h-0 flex-1 flex-col overflow-y-auto"
      >
        <MessageList messages={state.messages} />
      </div>
      {state.notice !== null && (
        <p className="text-sm text-amber-700 dark:text-amber-400">
          {NOTICE_TEXT[state.notice]}
        </p>
      )}
      {state.error !== null && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {state.error.message}
        </p>
      )}
      <Composer
        streaming={streaming}
        canSend={selected !== "" && !state.loading}
        onSend={(text) => send(selected, text)}
        onStop={stop}
      />
    </div>
  );
};

export { ChatView };
