import { ArrowUp, GitCompare, Square } from "lucide-react";
import type { KeyboardEvent, ReactNode, SubmitEvent } from "react";
import { useLayoutEffect, useRef, useState } from "react";

import { sendsMessage } from "~/features/chat/chat-keys";
import { ComparePicker } from "~/features/chat/compare-picker";

/** Sending the draft to several models instead; saved chats only. */
type CompareOption = {
  /** The installed chat models. */
  models: string[];
  /** Preselected in the picker. */
  current: string;
  /** Returns `false` when the comparison was refused, keeping the draft. */
  onCompare: (text: string, models: string[]) => boolean;
};

type ComposerProps = {
  streaming: boolean;
  /** False while no model is selected. */
  canSend: boolean;
  /** Returns `false` when the send was refused, keeping the draft. */
  onSend: (text: string) => boolean;
  onStop: () => void;
  /** A temporary chat: drawn with a dashed border. */
  temporary?: boolean;
  compare?: CompareOption;
  /** Controls for the next turn, left of Compare. */
  toolbar?: ReactNode;
};

const button =
  "flex size-9 shrink-0 items-center justify-center rounded-full text-white";

const Composer = ({
  streaming,
  canSend,
  onSend,
  onStop,
  temporary = false,
  compare,
  toolbar,
}: ComposerProps) => {
  const [draft, setDraft] = useState("");
  const [picking, setPicking] = useState(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const text = draft.trim();
  const sendable = canSend && text !== "";
  const comparable =
    compare !== undefined &&
    sendable &&
    !streaming &&
    compare.models.length > 1;

  // Grow with the draft up to `max-h-48`, then scroll; shrink after a send.
  useLayoutEffect(() => {
    const element = textarea.current;
    if (element !== null) {
      element.style.height = "auto";
      element.style.height = `${String(element.scrollHeight)}px`;
    }
  }, [draft]);

  const submit = () => {
    if (!sendable) {
      return;
    }
    if (onSend(text)) {
      setDraft("");
    }
  };

  const onSubmit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    submit();
  };

  // Esc is handled by the chat view, so it works wherever focus is.
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      sendsMessage({
        key: event.key,
        shiftKey: event.shiftKey,
        isComposing: event.nativeEvent.isComposing,
      })
    ) {
      event.preventDefault();
      submit();
    }
  };

  return (
    <div className="flex flex-col gap-2">
      {picking && compare !== undefined && (
        <ComparePicker
          models={compare.models}
          current={compare.current}
          onCancel={() => {
            setPicking(false);
          }}
          onCompare={(models) => {
            setPicking(false);
            if (comparable && compare.onCompare(text, models)) {
              setDraft("");
            }
          }}
        />
      )}
      <form
        onSubmit={onSubmit}
        className={`flex flex-col gap-1 rounded-2xl border border-zinc-300 bg-white p-2 shadow-sm has-[textarea:focus]:ring-2 has-[textarea:focus]:ring-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 ${temporary ? "border-dashed" : ""}`}
      >
        <label htmlFor="composer" className="sr-only">
          Message
        </label>
        <textarea
          id="composer"
          ref={textarea}
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
          }}
          onKeyDown={onKeyDown}
          rows={1}
          placeholder="Send a message (Enter to send, Shift+Enter for a new line, Esc to stop)"
          // No outline of its own: the form's ring shows the focus.
          className="max-h-48 resize-none overflow-y-auto bg-transparent px-2 py-2 text-sm outline-none"
        />
        <div className="flex items-center gap-1">
          {toolbar}
          {compare !== undefined && (
            <button
              type="button"
              onClick={() => {
                if (comparable) {
                  setPicking((open) => !open);
                }
              }}
              aria-label="Compare models"
              title="Ask 2 or 3 models and keep the answer you prefer"
              aria-expanded={picking}
              aria-disabled={comparable ? undefined : true}
              className="flex size-9 shrink-0 items-center justify-center rounded-full text-zinc-600 hover:bg-zinc-100 aria-disabled:cursor-not-allowed aria-disabled:opacity-40 dark:text-zinc-300 dark:hover:bg-zinc-800"
            >
              <GitCompare aria-hidden="true" className="size-5" />
            </button>
          )}
          {/* One button for Send and Stop: swapping two would drop the focus to
          the page when a reply starts or ends. `aria-disabled` rather than
          `disabled` for the same reason; `submit` refuses on its own. */}
          <button
            type={streaming ? "button" : "submit"}
            onClick={(event) => {
              if (streaming) {
                // Stopping re-renders this as a submit button before the click's
                // default action runs, which would send the draft.
                event.preventDefault();
                onStop();
              }
            }}
            aria-label={streaming ? "Stop reply" : "Send message"}
            title={streaming ? "Stop reply (Esc)" : "Send message (Enter)"}
            aria-keyshortcuts={streaming ? "Escape" : undefined}
            aria-disabled={streaming || sendable ? undefined : true}
            className={`${button} ml-auto ${streaming ? "bg-red-600 hover:bg-red-700" : "bg-zinc-900 hover:bg-zinc-700 aria-disabled:cursor-not-allowed aria-disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"}`}
          >
            {streaming ? (
              <Square aria-hidden="true" className="size-3.5 fill-current" />
            ) : (
              <ArrowUp aria-hidden="true" className="size-5" />
            )}
          </button>
        </div>
      </form>
    </div>
  );
};

export { Composer };
