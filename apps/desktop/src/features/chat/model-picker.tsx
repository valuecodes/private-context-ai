import { Brain, ChevronDown, Eye, Wrench } from "lucide-react";
import type { KeyboardEvent } from "react";
import { useId, useRef, useState } from "react";

import { Popover } from "~/components/popover";
import { filterModels } from "~/features/chat/model-filter";
import type { ModelCapabilities, ModelInfo } from "~/lib/types";

const BADGES = [
  { key: "thinking", label: "Thinking", Icon: Brain },
  { key: "vision", label: "Vision", Icon: Eye },
  { key: "tools", label: "Tools", Icon: Wrench },
] as const;

/** The index `delta` steps from `current`, wrapping; -1 for an empty list. */
const nextIndex = (current: number, delta: number, count: number): number => {
  if (count === 0) {
    return -1;
  }
  if (current < 0) {
    return delta > 0 ? 0 : count - 1;
  }
  return (current + delta + count) % count;
};

/** `current` while it is still shown, else the first shown model. */
const activeName = (
  visible: ModelInfo[],
  current: string | null
): string | null =>
  visible.some(({ name }) => name === current)
    ? current
    : (visible[0]?.name ?? null);

type ModelOptionsProps = {
  /** The listbox's id; option ids are derived from it. */
  id: string;
  /** The filtered models. */
  visible: ModelInfo[];
  /** Whether any chat model is installed at all. */
  installed: boolean;
  selected: string;
  active: string | null;
  capabilities: Record<string, ModelCapabilities>;
  onPick: (name: string) => void;
};

const optionId = (listId: string, index: number): string =>
  `${listId}-${String(index)}`;

/** Focuses the picker's search as the list opens; stable, so only once. */
const focusOnMount = (element: HTMLInputElement | null) => {
  element?.focus();
};

/** The picker's list: each model with its size and capability badges. */
const ModelOptions = ({
  id,
  visible,
  installed,
  selected,
  active,
  capabilities,
  onPick,
}: ModelOptionsProps) => {
  if (!installed) {
    return (
      <p className="px-3 py-2 text-zinc-500 dark:text-zinc-400">
        No chat models installed. Pull one with Ollama, then send a message to
        start.
      </p>
    );
  }
  if (visible.length === 0) {
    return (
      <p className="px-3 py-2 text-zinc-500 dark:text-zinc-400">
        No matching models
      </p>
    );
  }
  return (
    // `relative` keeps the badges' sr-only text inside this scroller.
    <ul
      id={id}
      role="listbox"
      aria-label="Models"
      className="relative max-h-[min(18rem,50vh)] overflow-y-auto py-1"
    >
      {visible.map((info, index) => {
        const caps = capabilities[info.name];
        return (
          <li
            key={info.name}
            id={optionId(id, index)}
            role="option"
            aria-selected={info.name === selected}
            tabIndex={-1}
            onClick={() => {
              onPick(info.name);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                onPick(info.name);
              }
            }}
            className={`flex cursor-pointer items-center gap-2 px-3 py-1.5 aria-selected:font-medium ${info.name === active ? "bg-zinc-100 dark:bg-zinc-800" : ""}`}
          >
            <span className="min-w-0 flex-1 truncate">{info.name}</span>
            {info.parameterSize !== null && (
              <span className="text-xs text-zinc-500 dark:text-zinc-400">
                {info.parameterSize}
              </span>
            )}
            {BADGES.filter(({ key }) => caps?.[key] === true).map(
              ({ key, label, Icon }) => (
                <span
                  key={key}
                  title={label}
                  className="text-zinc-500 dark:text-zinc-400"
                >
                  <Icon aria-hidden="true" className="size-3.5" />
                  <span className="sr-only">{label}</span>
                </span>
              )
            )}
          </li>
        );
      })}
    </ul>
  );
};

type ModelPickerProps = {
  /** The installed chat models. */
  models: ModelInfo[];
  capabilities: Record<string, ModelCapabilities>;
  /** `""` when no model is installed. */
  selected: string;
  onSelect: (name: string) => void;
  /** The list opened: look up the models not known yet. */
  onOpen: () => void;
  disabled: boolean;
};

/** The composer's model pill and its searchable list. */
const ModelPicker = ({
  models,
  capabilities,
  selected,
  onSelect,
  onOpen,
  disabled,
}: ModelPickerProps) => {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState<string | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const id = useId();
  const visible = filterModels(models, query);
  const active = activeName(visible, current);
  const activeIndex = visible.findIndex(({ name }) => name === active);
  const info = models.find(({ name }) => name === selected);

  const onOpenChange = (next: boolean) => {
    setOpen(next);
    if (next) {
      setQuery("");
      setCurrent(selected === "" ? null : selected);
      onOpen();
    }
  };

  return (
    <Popover
      open={open}
      onOpenChange={onOpenChange}
      label={info === undefined ? "No models" : `Model: ${info.name}`}
      title="Choose the model"
      popup="listbox"
      disabled={disabled}
      anchorClassName="flex max-w-56 items-center gap-1 rounded-full px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-zinc-800"
      anchor={
        <>
          <span className="truncate">
            {info === undefined ? "No models" : info.name}
          </span>
          {info !== undefined && info.parameterSize !== null && (
            <span className="text-xs text-zinc-500 dark:text-zinc-400">
              {info.parameterSize}
            </span>
          )}
          <ChevronDown aria-hidden="true" className="size-4 shrink-0" />
        </>
      }
      panelClassName="w-80"
    >
      {(close) => {
        const pick = (name: string) => {
          onSelect(name);
          close();
        };
        const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            const index = nextIndex(
              activeIndex,
              event.key === "ArrowDown" ? 1 : -1,
              visible.length
            );
            const next = visible[index];
            if (next !== undefined) {
              setCurrent(next.name);
              list.current
                ?.querySelector(`#${CSS.escape(optionId(id, index))}`)
                ?.scrollIntoView({ block: "nearest" });
            }
          } else if (event.key === "Enter") {
            // Inside the composer's form: Enter must not send the draft.
            event.preventDefault();
            if (active !== null && !event.nativeEvent.isComposing) {
              pick(active);
            }
          }
        };
        return (
          <div ref={list} className="flex flex-col">
            <div className="border-b border-zinc-200 p-2 dark:border-zinc-700">
              <input
                ref={focusOnMount}
                type="search"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                }}
                onKeyDown={onKeyDown}
                placeholder="Search models"
                aria-label="Search models"
                role="combobox"
                aria-expanded
                aria-controls={id}
                aria-activedescendant={
                  activeIndex < 0 ? undefined : optionId(id, activeIndex)
                }
                className="w-full rounded-md border border-zinc-300 bg-transparent px-2 py-1 outline-none focus:ring-2 focus:ring-zinc-400 dark:border-zinc-700"
              />
            </div>
            <ModelOptions
              id={id}
              visible={visible}
              installed={models.length > 0}
              selected={selected}
              active={active}
              capabilities={capabilities}
              onPick={pick}
            />
          </div>
        );
      }}
    </Popover>
  );
};

export { activeName, ModelOptions, ModelPicker, nextIndex };
