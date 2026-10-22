import type { CompareState, UiMessage } from "~/features/chat/chat-reducer";
import { AssistantReply } from "~/features/chat/message-list";

/** Columns side by side from this many on wide windows; stacked below. */
const COLUMNS: Record<number, string> = {
  2: "sm:grid-cols-2",
  3: "sm:grid-cols-3",
};

type Progress = "answering" | "waiting" | "done";

const STATUS_NOTE: Record<UiMessage["status"], string> = {
  complete: "",
  streaming: "",
  cancelled: "Stopped",
  error: "Failed",
};

type CompareViewProps = {
  compare: CompareState;
  /** The models are still answering: nothing can be kept or discarded yet. */
  comparing: boolean;
  onPick: (messageId: string) => void;
  onDiscard: () => void;
};

const Candidate = ({
  candidate,
  progress,
  pickable,
  onPick,
}: {
  candidate: UiMessage;
  progress: Progress;
  pickable: boolean;
  onPick: (messageId: string) => void;
}) => {
  const waiting = progress === "waiting";
  const note = waiting ? "Waiting" : STATUS_NOTE[candidate.status];
  return (
    <li className="flex min-w-0 flex-col gap-2 rounded-2xl border border-zinc-200 p-3 dark:border-zinc-800">
      <span className="text-xs font-medium text-zinc-600 dark:text-zinc-300">
        {candidate.model}
      </span>
      <div className="min-w-0 text-sm">
        {!waiting && <AssistantReply message={candidate} />}
      </div>
      {note !== "" && (
        <span className="text-xs text-zinc-500 dark:text-zinc-400">{note}</span>
      )}
      {candidate.status === "complete" && (
        <button
          type="button"
          disabled={!pickable}
          onClick={() => {
            onPick(candidate.id);
          }}
          className="mt-auto self-start rounded-full bg-zinc-900 px-3 py-1 text-sm text-white hover:bg-zinc-700 disabled:cursor-not-allowed disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
        >
          Continue with this
        </button>
      )}
    </li>
  );
};

/**
 * A compared prompt and one column per model. Once every model has
 * answered, one complete answer is kept or the comparison is discarded.
 */
const CompareView = ({
  compare,
  comparing,
  onPick,
  onDiscard,
}: CompareViewProps) => {
  const { prompt, candidates } = compare;
  const running = comparing
    ? candidates.findIndex(({ status }) => status === "streaming")
    : -1;
  const current = running === -1 ? undefined : candidates.at(running);
  const left = running === -1 ? 0 : candidates.length - running - 1;
  // Ids are Rust's once the group is known; a pick needs them.
  const settled = !comparing && compare.group !== null;
  const progress = (candidate: UiMessage, index: number): Progress => {
    if (candidate.status !== "streaming" || !comparing) {
      return "done";
    }
    return index === running ? "answering" : "waiting";
  };
  return (
    <section aria-label="Compared answers" className="flex flex-col gap-3">
      <div className="flex justify-end">
        <p className="max-w-[80%] rounded-2xl bg-zinc-200 px-4 py-2 text-sm whitespace-pre-wrap dark:bg-zinc-800">
          <span className="sr-only">You: </span>
          {prompt.content}
        </p>
      </div>
      <div className="flex items-center gap-2 text-xs text-zinc-500 dark:text-zinc-400">
        <span role="status">
          {comparing && current !== undefined
            ? `Answering with ${current.model ?? "a model"} · ${String(left)} left`
            : "Keep the answer you prefer, or discard them all."}
        </span>
        <button
          type="button"
          disabled={!settled}
          onClick={onDiscard}
          className="ml-auto rounded-full border border-zinc-300 px-3 py-1 text-sm text-zinc-700 hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-zinc-700 dark:text-zinc-200 dark:hover:bg-zinc-800"
        >
          Discard
        </button>
      </div>
      <ol
        aria-label="Answers"
        className={`grid grid-cols-1 gap-3 ${COLUMNS[candidates.length] ?? ""}`}
      >
        {candidates.map((candidate, index) => (
          <Candidate
            key={candidate.id}
            candidate={candidate}
            progress={progress(candidate, index)}
            pickable={settled}
            onPick={onPick}
          />
        ))}
      </ol>
    </section>
  );
};

export { CompareView };
