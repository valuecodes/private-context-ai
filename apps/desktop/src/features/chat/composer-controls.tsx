import { ChartColumn } from "lucide-react";

import { ModelPicker } from "~/features/chat/model-picker";
import { ThinkControl } from "~/features/chat/think-control";
import type {
  ConversationSettings,
  ModelCapabilities,
  ModelInfo,
} from "~/lib/types";

type ComposerControlsProps = {
  /** The installed chat models. */
  models: ModelInfo[];
  capabilities: Record<string, ModelCapabilities>;
  /** `""` when no model is installed. */
  selected: string;
  onSelect: (name: string) => void;
  /** The model list opened: look up every model not known yet. */
  onOpenPicker: () => void;
  settings: ConversationSettings;
  /** Applied from the next send. */
  onSettings: (settings: ConversationSettings) => void;
  /** While the chat loads, its saved settings are not known yet. */
  disabled: boolean;
};

/** How the next turn is set up: model, Instant/Thinking and Charts. */
const ComposerControls = ({
  models,
  capabilities,
  selected,
  onSelect,
  onOpenPicker,
  settings,
  onSettings,
  disabled,
}: ComposerControlsProps) => {
  const caps: ModelCapabilities | undefined = capabilities[selected];
  const tools = caps?.tools === true;
  return (
    <>
      <ModelPicker
        models={models}
        capabilities={capabilities}
        selected={selected}
        onSelect={onSelect}
        onOpen={onOpenPicker}
        disabled={disabled}
      />
      <ThinkControl
        caps={caps}
        choice={settings.think}
        onChange={(think) => {
          onSettings({ ...settings, think });
        }}
        disabled={disabled}
      />
      <button
        type="button"
        aria-label="Charts"
        aria-pressed={settings.charts}
        disabled={disabled || !tools}
        title={
          tools
            ? "Lets the model draw bar, line and pie charts. Nothing is run or fetched."
            : "This model can't draw charts: it doesn't support tools."
        }
        onClick={() => {
          onSettings({ ...settings, charts: !settings.charts });
        }}
        className="flex size-8 shrink-0 items-center justify-center rounded-full text-zinc-600 hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 aria-pressed:bg-zinc-900 aria-pressed:text-white dark:text-zinc-300 dark:hover:bg-zinc-800 dark:aria-pressed:bg-zinc-100 dark:aria-pressed:text-zinc-900"
      >
        <ChartColumn aria-hidden="true" className="size-4" />
      </button>
    </>
  );
};

export { ComposerControls };
