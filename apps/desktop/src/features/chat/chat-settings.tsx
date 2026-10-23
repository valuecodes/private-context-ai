import { RotateCcw, SlidersHorizontal } from "lucide-react";

import { ThinkControl } from "~/features/chat/think-control";
import type {
  ChatOptions,
  ConversationSettings,
  ModelCapabilities,
} from "~/lib/types";

/** Where the slider sits while the model's own temperature applies. */
const DEFAULT_TEMPERATURE = 0.8;

const CONTEXT_SIZES = [2048, 4096, 8192, 16_384, 32_768];

/** `8192` → `8k`. */
const contextLabel = (tokens: number): string =>
  tokens % 1024 === 0 ? `${tokens / 1024}k` : String(tokens);

type ChatSettingsProps = {
  settings: ConversationSettings;
  onChange: (settings: ConversationSettings) => void;
  /** The selected model's; the Think control shows once they are known. */
  caps: ModelCapabilities | undefined;
  /** While the chat loads, its saved settings are not known yet. */
  disabled: boolean;
};

/**
 * The open chat's settings: system prompt, temperature, context size, Think
 * and Charts. Each change applies from the next send.
 */
const ChatSettingsPanel = ({
  settings,
  onChange,
  caps,
  disabled,
}: ChatSettingsProps) => {
  const { temperature, num_ctx: numCtx } = settings.options;
  // A size saved outside the list still shows as selected.
  const sizes =
    numCtx === undefined || CONTEXT_SIZES.includes(numCtx)
      ? CONTEXT_SIZES
      : [...CONTEXT_SIZES, numCtx].toSorted((a, b) => a - b);
  // An unset option is left out of the IPC payload, so Rust sees none.
  const setOption = (options: ChatOptions) => {
    onChange({ ...settings, options: { ...settings.options, ...options } });
  };

  return (
    <fieldset disabled={disabled} className="flex flex-col gap-4 text-sm">
      <div className="flex flex-col gap-1">
        <label htmlFor="system-prompt" className="font-medium">
          System prompt
        </label>
        {/* Saved when it loses focus; remounted when the saved text changes. */}
        <textarea
          key={settings.systemPrompt}
          id="system-prompt"
          defaultValue={settings.systemPrompt}
          rows={4}
          placeholder="How the model should answer in this chat"
          onBlur={(event) => {
            const systemPrompt = event.currentTarget.value;
            if (systemPrompt !== settings.systemPrompt) {
              onChange({ ...settings, systemPrompt });
            }
          }}
          className="resize-y rounded border border-zinc-300 bg-white px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
        />
      </div>
      <div className="flex flex-col gap-1">
        <div className="flex items-center justify-between">
          <label htmlFor="temperature" className="font-medium">
            Temperature
          </label>
          <span className="flex items-center gap-1 text-zinc-500 dark:text-zinc-400">
            {temperature === undefined ? "Default" : temperature.toFixed(1)}
            {temperature !== undefined && (
              <button
                type="button"
                title="Use the model's default"
                aria-label="Reset temperature"
                onClick={() => {
                  setOption({ temperature: undefined });
                }}
                className="rounded p-0.5 hover:bg-zinc-100 dark:hover:bg-zinc-800"
              >
                <RotateCcw aria-hidden="true" className="size-3.5" />
              </button>
            )}
          </span>
        </div>
        <input
          id="temperature"
          type="range"
          min={0}
          max={2}
          step={0.1}
          value={temperature ?? DEFAULT_TEMPERATURE}
          onChange={(event) => {
            setOption({ temperature: Number(event.currentTarget.value) });
          }}
        />
      </div>
      <div className="flex flex-col gap-1">
        <label htmlFor="num-ctx" className="font-medium">
          Context size
        </label>
        <select
          id="num-ctx"
          value={numCtx ?? ""}
          onChange={(event) => {
            const { value } = event.currentTarget;
            setOption({ num_ctx: value === "" ? undefined : Number(value) });
          }}
          className="rounded border border-zinc-300 bg-white px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
        >
          <option value="">Model default</option>
          {sizes.map((tokens) => (
            <option key={tokens} value={tokens}>
              {`${contextLabel(tokens)} tokens`}
            </option>
          ))}
        </select>
        <p className="text-xs text-zinc-500 dark:text-zinc-400">
          Larger contexts use more memory and make the model slower to load.
        </p>
      </div>
      {caps?.thinking === true && (
        <div className="flex flex-col gap-1">
          <span className="font-medium">Thinking</span>
          <ThinkControl
            caps={caps}
            choice={settings.think}
            onChange={(think) => {
              onChange({ ...settings, think });
            }}
          />
        </div>
      )}
      <div className="flex flex-col gap-1">
        <label className="flex items-center gap-2 font-medium">
          <input
            id="charts"
            type="checkbox"
            checked={settings.charts}
            disabled={caps?.tools !== true}
            aria-describedby="charts-hint"
            onChange={(event) => {
              onChange({ ...settings, charts: event.currentTarget.checked });
            }}
          />
          Charts
        </label>
        <p
          id="charts-hint"
          className="text-xs text-zinc-500 dark:text-zinc-400"
        >
          {caps?.tools === true
            ? "Lets the model draw bar, line and pie charts. Nothing is run or fetched."
            : "This model can't draw charts: it doesn't support tools."}
        </p>
      </div>
    </fieldset>
  );
};

/** The chat header's settings button, opening the panel below it. */
const ChatSettings = (props: ChatSettingsProps) => (
  <details className="group relative">
    <summary
      aria-label="Chat settings"
      title="Chat settings"
      className="flex cursor-pointer list-none items-center rounded border border-zinc-300 px-2 py-1 group-open:bg-zinc-100 hover:bg-zinc-100 dark:border-zinc-700 dark:group-open:bg-zinc-800 dark:hover:bg-zinc-800"
    >
      <SlidersHorizontal aria-hidden="true" className="size-4" />
    </summary>
    <div className="absolute right-0 z-10 mt-1 w-80 rounded border border-zinc-200 bg-white p-3 shadow-lg dark:border-zinc-700 dark:bg-zinc-900">
      <ChatSettingsPanel {...props} />
    </div>
  </details>
);

export { ChatSettings, ChatSettingsPanel };
