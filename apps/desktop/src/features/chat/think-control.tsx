import { Brain, Check, ChevronDown } from "lucide-react";
import { useState } from "react";

import { Popover } from "~/components/popover";
import { selectedLevel, THINK_OFF } from "~/features/chat/think-choice";
import type { ModelCapabilities, ThinkChoice, ThinkLevel } from "~/lib/types";

/** A level as Ollama names it, capitalized: `xhigh` → `Xhigh`. */
const levelLabel = (level: ThinkLevel): string =>
  level.charAt(0).toUpperCase() + level.slice(1);

const segment =
  "rounded-full px-2.5 py-1 aria-pressed:bg-zinc-900 aria-pressed:text-white dark:aria-pressed:bg-zinc-100 dark:aria-pressed:text-zinc-900";

type ThinkLevelMenuProps = {
  /** `null` is Instant. */
  options: (ThinkLevel | null)[];
  selected: ThinkLevel | null;
  onPick: (option: ThinkLevel | null) => void;
};

/** Instant, when the model allows it, and the model's levels. */
const ThinkLevelMenu = ({ options, selected, onPick }: ThinkLevelMenuProps) => (
  <div role="menu" aria-label="Thinking level" className="flex flex-col py-1">
    {options.map((option) => (
      <button
        // Rust drops empty level names, so "" is free for Instant.
        key={option ?? ""}
        type="button"
        role="menuitemradio"
        aria-checked={selected === option}
        onClick={() => {
          onPick(option);
        }}
        className="flex items-center gap-2 px-3 py-1.5 text-left hover:bg-zinc-100 dark:hover:bg-zinc-800"
      >
        <Check
          aria-hidden="true"
          className={`size-4 ${selected === option ? "" : "invisible"}`}
        />
        {option === null ? "Instant" : levelLabel(option)}
      </button>
    ))}
  </div>
);

type ThinkControlProps = {
  /** The selected model's; nothing shows until they are known. */
  caps: ModelCapabilities | undefined;
  choice: ThinkChoice;
  onChange: (choice: ThinkChoice) => void;
  disabled: boolean;
};

/**
 * Instant or Thinking for a thinking model: two segments for on/off models,
 * a level menu for models that take one. Nothing for other models.
 */
const ThinkControl = ({
  caps,
  choice,
  onChange,
  disabled,
}: ThinkControlProps) => {
  const [open, setOpen] = useState(false);
  if (caps === undefined || !caps.thinking) {
    return null;
  }
  const style = caps.thinkStyle;
  if (style.kind === "bool") {
    const on = choice.kind !== "off";
    return (
      <div
        role="group"
        aria-label="Thinking"
        title="Let the model think before it answers"
        className="flex items-center rounded-full border border-zinc-300 p-0.5 text-xs dark:border-zinc-700"
      >
        <button
          type="button"
          aria-pressed={!on}
          disabled={disabled}
          onClick={() => {
            onChange(THINK_OFF);
          }}
          className={segment}
        >
          Instant
        </button>
        <button
          type="button"
          aria-pressed={on}
          disabled={disabled}
          onClick={() => {
            onChange({ kind: "on" });
          }}
          className={segment}
        >
          Thinking
        </button>
      </div>
    );
  }
  const selected = selectedLevel(caps, choice);
  // `null` is Instant, so a level a model happens to call "off" stays
  // distinct.
  const options: (ThinkLevel | null)[] = style.canDisable
    ? [null, ...style.levels]
    : style.levels;
  const label =
    selected === null ? "Instant" : `Thinking: ${levelLabel(selected)}`;
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      label={label}
      title={
        style.canDisable
          ? "How much the model thinks before it answers"
          : "This model always thinks"
      }
      popup="menu"
      disabled={disabled}
      anchorClassName="flex items-center gap-1 rounded-full px-2.5 py-1.5 text-xs hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-zinc-800"
      anchor={
        <>
          <Brain aria-hidden="true" className="size-4" />
          {label}
          <ChevronDown aria-hidden="true" className="size-3.5" />
        </>
      }
      panelClassName="w-44"
    >
      {(close) => (
        <ThinkLevelMenu
          options={options}
          selected={selected}
          onPick={(option) => {
            onChange(
              option === null ? THINK_OFF : { kind: "level", level: option }
            );
            close();
          }}
        />
      )}
    </Popover>
  );
};

export { ThinkControl, ThinkLevelMenu };
