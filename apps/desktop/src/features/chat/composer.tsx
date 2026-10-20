import type { KeyboardEvent, SubmitEvent } from "react";
import { useState } from "react";

type ComposerProps = {
  streaming: boolean;
  /** False while no model is selected. */
  canSend: boolean;
  /** Returns `false` when the send was refused, keeping the draft. */
  onSend: (text: string) => boolean;
  onStop: () => void;
};

const Composer = ({ streaming, canSend, onSend, onStop }: ComposerProps) => {
  const [draft, setDraft] = useState("");
  const text = draft.trim();

  const submit = () => {
    if (text === "" || !canSend) {
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

  // Enter sends, Shift+Enter starts a new line.
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      event.key === "Enter" &&
      !event.shiftKey &&
      !event.nativeEvent.isComposing
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
        placeholder="Send a message (Enter to send, Shift+Enter for a new line)"
        className="flex-1 resize-none rounded-lg border border-zinc-300 bg-white p-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
      />
      {streaming ? (
        <button
          type="button"
          onClick={onStop}
          className="rounded-lg bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700"
        >
          Stop
        </button>
      ) : (
        <button
          type="submit"
          disabled={!canSend || text === ""}
          className="rounded-lg bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-700 disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
        >
          Send
        </button>
      )}
    </form>
  );
};

export { Composer };
