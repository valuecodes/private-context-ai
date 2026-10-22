import { useId, useState } from "react";

const MIN_MODELS = 2;
const MAX_MODELS = 3;

/** `picked` with `model` turned on or off; never more than three. */
const togglePick = (
  picked: ReadonlySet<string>,
  model: string,
  on: boolean
): ReadonlySet<string> => {
  if (on && picked.size >= MAX_MODELS) {
    return picked;
  }
  const next = new Set(picked);
  if (on) {
    next.add(model);
  } else {
    next.delete(model);
  }
  return next;
};

/** Two or three models can be compared. */
const canCompare = (picked: ReadonlySet<string>): boolean =>
  picked.size >= MIN_MODELS && picked.size <= MAX_MODELS;

type ComparePickerProps = {
  /** The installed chat models. */
  models: string[];
  /** Preselected. */
  current: string;
  /** Two or three models, in the list's order; they answer in that order. */
  onCompare: (models: string[]) => void;
  onCancel: () => void;
};

/** Picks the models a prompt is compared across. */
const ComparePicker = ({
  models,
  current,
  onCompare,
  onCancel,
}: ComparePickerProps) => {
  const [picked, setPicked] = useState<ReadonlySet<string>>(
    () => new Set(models.includes(current) ? [current] : [])
  );
  const id = useId();
  const full = picked.size >= MAX_MODELS;
  const ready = canCompare(picked);

  const toggle = (model: string, on: boolean) => {
    setPicked((before) => togglePick(before, model, on));
  };

  return (
    <fieldset
      aria-describedby={`${id}-hint`}
      className="flex flex-col gap-2 rounded-2xl border border-zinc-300 bg-white p-3 text-sm dark:border-zinc-700 dark:bg-zinc-900"
    >
      <legend className="px-1 font-medium">Compare models</legend>
      <p id={`${id}-hint`} className="text-xs text-zinc-500 dark:text-zinc-400">
        Pick 2 or 3. They answer one after another; on a laptop this takes a
        while.
      </p>
      {models.map((model) => {
        const checked = picked.has(model);
        return (
          <label key={model} className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={checked}
              disabled={!checked && full}
              onChange={(event) => {
                toggle(model, event.target.checked);
              }}
            />
            {model}
          </label>
        );
      })}
      <div className="flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-full border border-zinc-300 px-3 py-1 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={!ready}
          onClick={() => {
            onCompare(models.filter((model) => picked.has(model)));
          }}
          className="rounded-full bg-zinc-900 px-3 py-1 text-white hover:bg-zinc-700 disabled:cursor-not-allowed disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
        >
          Compare
        </button>
      </div>
    </fieldset>
  );
};

export { canCompare, ComparePicker, togglePick };
