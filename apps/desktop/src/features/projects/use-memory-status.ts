import { useCallback, useEffect, useRef, useState } from "react";

import { onMemoryStatus, projectMemoryPending } from "~/lib/ipc";
import type { MemoryStatus } from "~/lib/types";

/** How long "Memory updated" stays. */
const SAVED_MS = 4000;

/** What one project's background memory is doing, for its indicators. */
type ProjectMemoryStatus = {
  /** Chats whose replies are not in memory yet. */
  pending: number;
  /** The chat `state` is about. */
  conversationId: string | null;
  state: "running" | "saved" | "failed" | null;
};

/** By project id. */
type MemoryStatuses = Readonly<Record<string, ProjectMemoryStatus>>;

const quiet: ProjectMemoryStatus = {
  pending: 0,
  conversationId: null,
  state: null,
};

/**
 * The statuses after `status`: running, saved and failed are shown; queued
 * and cancelled clear what was shown for that chat.
 */
const applyStatus = (
  statuses: MemoryStatuses,
  status: MemoryStatus
): MemoryStatuses => {
  const current = statuses[status.projectId] ?? quiet;
  const next = { ...current, pending: status.pending };
  if (
    status.state === "running" ||
    status.state === "saved" ||
    status.state === "failed"
  ) {
    next.state = status.state;
    next.conversationId = status.conversationId;
  } else if (current.conversationId === status.conversationId) {
    next.state = null;
    next.conversationId = null;
  }
  return { ...statuses, [status.projectId]: next };
};

/** Takes "saved" away again, unless something newer replaced it. */
const clearSaved = (
  statuses: MemoryStatuses,
  projectId: string,
  conversationId: string
): MemoryStatuses => {
  const current = statuses[projectId];
  if (current?.state !== "saved" || current.conversationId !== conversationId) {
    return statuses;
  }
  return {
    ...statuses,
    [projectId]: { ...current, state: null, conversationId: null },
  };
};

/** Replaces every project's pending count with Rust's. */
const withCounts = (
  statuses: MemoryStatuses,
  counts: { projectId: string; pending: number }[]
): MemoryStatuses => {
  const next: Record<string, ProjectMemoryStatus> = {};
  for (const [projectId, status] of Object.entries(statuses)) {
    next[projectId] = { ...status, pending: 0 };
  }
  for (const { projectId, pending } of counts) {
    next[projectId] = { ...(next[projectId] ?? quiet), pending };
  }
  return next;
};

/**
 * Background memory per project, from the `memory-status` events. Counts
 * start from `projectMemoryPending` and are read again on `reload`.
 */
const useMemoryStatus = () => {
  const [statuses, setStatuses] = useState<MemoryStatuses>({});
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());

  const reload = useCallback(() => {
    projectMemoryPending().then(
      (counts) => {
        setStatuses((current) => withCounts(current, counts));
      },
      () => {
        // Best effort: the next event carries the count again.
      }
    );
  }, []);

  useEffect(() => {
    const pending = timers.current;
    let unlisten: (() => void) | null = null;
    let stopped = false;
    onMemoryStatus((status) => {
      setStatuses((current) => applyStatus(current, status));
      if (status.state === "saved") {
        const timer = setTimeout(() => {
          pending.delete(timer);
          setStatuses((current) =>
            clearSaved(current, status.projectId, status.conversationId)
          );
        }, SAVED_MS);
        pending.add(timer);
      }
    }).then(
      (stop) => {
        if (stopped) {
          stop();
        } else {
          unlisten = stop;
        }
      },
      () => {
        // Without events the counts still come from `reload`.
      }
    );
    return () => {
      stopped = true;
      unlisten?.();
      for (const timer of pending) {
        clearTimeout(timer);
      }
      pending.clear();
    };
  }, []);

  return { statuses, reload };
};

export type { MemoryStatuses, ProjectMemoryStatus };
export { applyStatus, clearSaved, useMemoryStatus, withCounts };
