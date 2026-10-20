import type { StatusState } from "~/features/status/status-state";
import { describeStatus } from "~/features/status/status-state";

type OllamaStatusProps = {
  status: StatusState;
  onRetry: () => void;
};

const OllamaStatus = ({ status, onRetry }: OllamaStatusProps) => {
  const { text, canRetry } = describeStatus(status);
  return (
    <div className="flex items-center gap-3">
      <p role="status" className="text-sm text-zinc-500 dark:text-zinc-400">
        {text}
      </p>
      {canRetry && (
        <button
          type="button"
          onClick={onRetry}
          className="rounded border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          Retry
        </button>
      )}
    </div>
  );
};

export { OllamaStatus };
