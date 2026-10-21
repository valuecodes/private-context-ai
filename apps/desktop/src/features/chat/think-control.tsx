import { Brain } from "lucide-react";

import { selectedLevel } from "~/features/chat/think-choice";
import type { ThinkChoice } from "~/features/chat/think-choice";
import type { ModelCapabilities, ThinkLevel } from "~/lib/types";

const LEVEL_LABEL: Record<ThinkLevel | "off", string> = {
  off: "Off",
  low: "Low",
  medium: "Medium",
  high: "High",
};

type ThinkControlProps = {
  caps: ModelCapabilities;
  choice: ThinkChoice;
  onChange: (choice: ThinkChoice) => void;
};

/**
 * The Think switch for a thinking model: a toggle for on/off models, the
 * advertised levels for models that take one. Hidden for other models.
 */
const ThinkControl = ({ caps, choice, onChange }: ThinkControlProps) => {
  if (!caps.thinking) {
    return null;
  }
  const style = caps.thinkStyle;
  if (style.kind === "bool") {
    return (
      <button
        type="button"
        aria-pressed={choice.on}
        onClick={() => {
          onChange({ ...choice, on: !choice.on });
        }}
        title="Let the model think before it answers"
        className={`flex items-center gap-1 rounded border px-2 py-0.5 text-sm ${choice.on ? "border-zinc-900 bg-zinc-900 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900" : "border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"}`}
      >
        <Brain aria-hidden="true" className="size-4" />
        Think
      </button>
    );
  }
  const selected = selectedLevel(caps, choice);
  const options: (ThinkLevel | "off")[] = style.canDisable
    ? ["off", ...style.levels]
    : style.levels;
  return (
    <div className="flex items-center gap-2">
      <div
        role="radiogroup"
        aria-label="Thinking level"
        className="flex items-center overflow-hidden rounded border border-zinc-300 text-sm dark:border-zinc-700"
      >
        <Brain aria-hidden="true" className="mx-1.5 size-4" />
        {options.map((option) => (
          <button
            key={option}
            type="button"
            role="radio"
            aria-checked={selected === option}
            onClick={() => {
              onChange(
                option === "off"
                  ? { ...choice, on: false }
                  : { on: true, level: option }
              );
            }}
            className={`px-2 py-0.5 ${selected === option ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900" : "hover:bg-zinc-100 dark:hover:bg-zinc-800"}`}
          >
            {LEVEL_LABEL[option]}
          </button>
        ))}
      </div>
      {!style.canDisable && (
        <span className="text-xs text-zinc-500 dark:text-zinc-400">
          This model always thinks
        </span>
      )}
    </div>
  );
};

export { ThinkControl };
