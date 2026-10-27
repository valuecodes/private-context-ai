import { ChartColumn, FileText } from "lucide-react";

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
  /**
   * Whether the chat's project has files: the Files switch shows unless this
   * is `false`, so it stays reachable while that is not known (`null`).
   * `undefined` outside a project.
   */
  hasFiles?: boolean | null;
};

const toggle =
  "flex size-8 shrink-0 items-center justify-center rounded-full text-zinc-600 hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 aria-pressed:bg-zinc-900 aria-pressed:text-white dark:text-zinc-300 dark:hover:bg-zinc-800 dark:aria-pressed:bg-zinc-100 dark:aria-pressed:text-zinc-900";

/** How the next turn is set up: model, Instant/Thinking, Charts and Files. */
const ComposerControls = ({
  models,
  capabilities,
  selected,
  onSelect,
  onOpenPicker,
  settings,
  onSettings,
  disabled,
  hasFiles,
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
        className={toggle}
      >
        <ChartColumn aria-hidden="true" className="size-4" />
      </button>
      {hasFiles !== undefined && hasFiles !== false && (
        <button
          type="button"
          aria-label="Files"
          aria-pressed={settings.files}
          disabled={disabled}
          title="Adds the most relevant passages of this project's files to each message. Nothing leaves this computer."
          onClick={() => {
            onSettings({ ...settings, files: !settings.files });
          }}
          className={toggle}
        >
          <FileText aria-hidden="true" className="size-4" />
        </button>
      )}
    </>
  );
};

export { ComposerControls };
