import type { KeyboardEvent, SubmitEvent } from "react";
import { useState } from "react";

import { sendsMessage } from "~/features/chat/chat-keys";

type ComposerProps = {
  streaming: boolean;
  /** False while no model is selected. */
  canSend: boolean;
  /** Returns `false` when the send was refused, keeping the draft. */
  onSend: (text: string) => boolean;
  onStop: () => void;
};

const button = "rounded-lg px-4 py-2 text-sm font-medium text-white";

const Composer = ({ streaming, canSend, onSend, onStop }: ComposerProps) => {
  const [draft, setDraft] = useState("");
  const text = draft.trim();
  const sendable = canSend && text !== "";

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
    <form onSubmit={onSubmit} className="flex items-end gap-2">
      <label htmlFor="composer" className="sr-only">
        Message
      </label>
      <textarea
        id="composer"
        value={draft}
        onChange={(event) => {
          setDraft(event.target.value);
        }}
        onKeyDown={onKeyDown}
        rows={3}
        placeholder="Send a message (Enter to send, Shift+Enter for a new line, Esc to stop)"
        className="flex-1 resize-none rounded-lg border border-zinc-300 bg-white p-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
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
        aria-keyshortcuts={streaming ? "Escape" : undefined}
        aria-disabled={streaming || sendable ? undefined : true}
        className={`${button} ${streaming ? "bg-red-600 hover:bg-red-700" : "bg-zinc-900 hover:bg-zinc-700 aria-disabled:cursor-not-allowed aria-disabled:opacity-40 aria-disabled:hover:bg-zinc-900 dark:bg-zinc-100 dark:text-zinc-900"}`}
      >
        {streaming ? "Stop" : "Send"}
      </button>
    </form>
  );
};

export { Composer };
