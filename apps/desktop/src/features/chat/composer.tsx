import { ArrowUp, Square } from "lucide-react";
import type { KeyboardEvent, SubmitEvent } from "react";
import { useLayoutEffect, useRef, useState } from "react";

import { sendsMessage } from "~/features/chat/chat-keys";

type ComposerProps = {
  streaming: boolean;
  /** False while no model is selected. */
  canSend: boolean;
  /** Returns `false` when the send was refused, keeping the draft. */
  onSend: (text: string) => boolean;
  onStop: () => void;
  /** A temporary chat: drawn with a dashed border. */
  temporary?: boolean;
};

const button =
  "flex size-9 shrink-0 items-center justify-center rounded-full text-white";

const Composer = ({
  streaming,
  canSend,
  onSend,
  onStop,
  temporary = false,
}: ComposerProps) => {
  const [draft, setDraft] = useState("");
  const textarea = useRef<HTMLTextAreaElement>(null);
  const text = draft.trim();
  const sendable = canSend && text !== "";

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
    <form
      onSubmit={onSubmit}
      className={`flex items-end gap-2 rounded-3xl border border-zinc-300 bg-white p-2 pl-4 has-[textarea:focus]:ring-2 has-[textarea:focus]:ring-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 ${temporary ? "border-dashed" : ""}`}
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
        className="max-h-48 flex-1 resize-none overflow-y-auto bg-transparent py-2 text-sm outline-none"
      />
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
        className={`${button} ${streaming ? "bg-red-600 hover:bg-red-700" : "bg-zinc-900 hover:bg-zinc-700 aria-disabled:cursor-not-allowed aria-disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"}`}
      >
        {streaming ? (
          <Square aria-hidden="true" className="size-3.5 fill-current" />
        ) : (
          <ArrowUp aria-hidden="true" className="size-5" />
        )}
      </button>
    </form>
  );
};

export { Composer };
