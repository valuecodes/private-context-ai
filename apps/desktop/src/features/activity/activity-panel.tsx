import { useEffect, useState } from "react";

import {
  formatDuration,
  formatTime,
  formatTokens,
  hostsSummary,
  statusLabel,
} from "~/features/activity/activity";
import { toAppError } from "~/lib/app-error";
import { activityHosts, activityList } from "~/lib/ipc";
import type { AppError, AuditEvent } from "~/lib/types";

/** Rows per page. */
const PAGE = 200;

const cell = "px-2 py-1 text-left align-top";

type ActivityTableProps = {
  /** Newest first. */
  events: AuditEvent[];
  /** Every host in the retained rows, not only the loaded ones. */
  hosts: string[];
};

const ActivityTable = ({ events, hosts }: ActivityTableProps) => (
  <div className="flex min-h-0 flex-1 flex-col gap-2">
    <p className="text-sm font-medium">{hostsSummary(hosts)}</p>
    {events.length === 0 ? (
      <p className="text-sm text-zinc-500 dark:text-zinc-400">
        No requests recorded yet.
      </p>
    ) : (
      <div className="min-h-0 flex-1 overflow-auto">
        <table className="w-full text-xs">
          <caption className="sr-only">
            Requests and app events, newest first
          </caption>
          <thead className="border-b border-zinc-200 dark:border-zinc-800">
            <tr>
              <th className={cell}>Time</th>
              <th className={cell}>Kind</th>
              <th className={cell}>Host</th>
              <th className={cell}>Model</th>
              <th className={cell}>Tokens</th>
              <th className={cell}>Duration</th>
              <th className={cell}>Status</th>
            </tr>
          </thead>
          <tbody>
            {events.map((event) => (
              <tr
                key={event.id}
                className="border-b border-zinc-100 dark:border-zinc-900"
              >
                <td className={`${cell} whitespace-nowrap`}>
                  {formatTime(event.at)}
                </td>
                <td className={cell}>{event.kind}</td>
                <td className={cell}>{event.host ?? "—"}</td>
                <td className={cell}>{event.model ?? "—"}</td>
                <td className={cell}>{formatTokens(event)}</td>
                <td className={cell}>{formatDuration(event.durationMs)}</td>
                <td
                  className={`${cell} ${event.status === "ok" ? "" : "text-amber-700 dark:text-amber-400"}`}
                >
                  {statusLabel(event)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    )}
  </div>
);

type Loaded = {
  events: AuditEvent[];
  hosts: string[];
  /** False once a page came back short. */
  more: boolean;
};

const button =
  "rounded border border-zinc-300 px-2 py-0.5 text-xs hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";

/** Every request the app made, read from the encrypted audit log. */
const ActivityPanel = () => {
  const [loaded, setLoaded] = useState<Loaded>({
    events: [],
    hosts: [],
    more: false,
  });
  const [busy, setBusy] = useState(true);
  const [failure, setFailure] = useState<AppError | null>(null);
  // Bumped by "Refresh" to load the newest page again.
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([activityList(PAGE), activityHosts()]).then(
      ([events, hosts]) => {
        if (!cancelled) {
          setLoaded({ events, hosts, more: events.length === PAGE });
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

  const loadOlder = async () => {
    const last = loaded.events.at(-1);
    if (last === undefined) {
      return;
    }
    setBusy(true);
    try {
      const older = await activityList(PAGE, last.id);
      setLoaded((current) => ({
        ...current,
        events: [...current.events, ...older],
        more: older.length === PAGE,
      }));
      setFailure(null);
    } catch (error: unknown) {
      setFailure(toAppError(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      aria-labelledby="activity-heading"
      className="flex min-h-0 flex-1 flex-col gap-3 p-4"
    >
      <div className="flex items-center gap-2">
        <h2 id="activity-heading" className="text-sm font-semibold">
          Activity
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
      {failure !== null && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {failure.message}
        </p>
      )}
      <ActivityTable events={loaded.events} hosts={loaded.hosts} />
      {loaded.more && (
        <button
          type="button"
          onClick={() => {
            void loadOlder();
          }}
          disabled={busy}
          className={`${button} self-start`}
        >
          Load older
        </button>
      )}
    </section>
  );
};

export { ActivityPanel, ActivityTable };
