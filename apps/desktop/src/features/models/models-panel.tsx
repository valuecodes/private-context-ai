import { useEffect, useState } from "react";

import { formatTime } from "~/features/activity/activity";
import { ChartBlock } from "~/features/chat/chart-block";
import {
  formatLoad,
  formatTps,
  sortBySpeed,
  speedChart,
} from "~/features/models/model-stats";
import { toAppError } from "~/lib/app-error";
import { activityModelStats } from "~/lib/ipc";
import type { AppError, ModelStats } from "~/lib/types";

const cell = "px-2 py-1 text-left align-top";
const number = "px-2 py-1 text-right align-top tabular-nums";

type ModelStatsViewProps = {
  stats: ModelStats[];
};

/** The speed chart and table, fastest model first. */
const ModelStatsView = ({ stats }: ModelStatsViewProps) => {
  if (stats.length === 0) {
    return (
      <p className="text-sm text-zinc-500 dark:text-zinc-400">
        No speed data yet. Timings are recorded from this version on.
      </p>
    );
  }
  const sorted = sortBySpeed(stats);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto">
      <ChartBlock spec={speedChart(sorted)} />
      {/* `relative` contains the sr-only caption. */}
      <div className="relative">
        <table className="w-full text-xs">
          <caption className="sr-only">Speed per model, fastest first</caption>
          <thead className="border-b border-zinc-200 dark:border-zinc-800">
            <tr>
              <th className={cell}>Model</th>
              <th className={number}>Requests</th>
              <th className={number}>Median tok/s</th>
              <th className={number}>p90 tok/s</th>
              <th className={number}>Prompt tok/s</th>
              <th className={number}>Median load</th>
              <th className={cell}>Last used</th>
            </tr>
          </thead>
          <tbody>
            {sorted.map((entry) => (
              <tr
                key={entry.model}
                className="border-b border-zinc-100 dark:border-zinc-900"
              >
                <td className={`${cell} break-all`}>{entry.model}</td>
                <td className={number}>{entry.runs}</td>
                <td className={number}>{formatTps(entry.genTpsMedian)}</td>
                <td className={number}>{formatTps(entry.genTpsP90)}</td>
                <td className={number}>{formatTps(entry.promptTpsMedian)}</td>
                <td className={number}>{formatLoad(entry.loadMsMedian)}</td>
                <td className={`${cell} whitespace-nowrap`}>
                  {formatTime(entry.lastAt)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
};

const button =
  "rounded border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";

/** How fast each model has run, from Ollama's own timings in the audit log. */
const ModelsPanel = () => {
  const [stats, setStats] = useState<ModelStats[]>([]);
  const [busy, setBusy] = useState(true);
  const [failure, setFailure] = useState<AppError | null>(null);
  // Bumped by "Refresh" to load again.
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void activityModelStats().then(
      (loaded) => {
        if (!cancelled) {
          setStats(loaded);
          setFailure(null);
          setBusy(false);
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          setFailure(toAppError(error));
          setBusy(false);
        }
      }
    );
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  const refresh = () => {
    setBusy(true);
    setAttempt((count) => count + 1);
  };

  return (
    <section
      aria-labelledby="models-heading"
      className="flex min-h-0 flex-1 flex-col gap-3 p-4"
    >
      <div className="flex items-center gap-2">
        <h2 id="models-heading" className="text-sm font-semibold">
          Models
        </h2>
        <button
          type="button"
          onClick={refresh}
          disabled={busy}
          className={button}
        >
          Refresh
        </button>
      </div>
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        From the retained Activity log (the newest 10,000 events). Each Ollama
        request counts once; a reply with a chart makes two.
      </p>
      {failure !== null && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {failure.message}
        </p>
      )}
      <ModelStatsView stats={stats} />
    </section>
  );
};

export { ModelStatsView, ModelsPanel };
