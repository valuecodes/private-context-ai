import type { AuditEvent, AuditStatus } from "~/lib/types";

/**
 * The summary line. `hosts` comes from every retained audit row, so the line
 * never depends on which page of the table is loaded.
 */
const hostsSummary = (hosts: string[]): string =>
  `Hosts contacted: ${hosts.length === 0 ? "none" : hosts.join(", ")}`;

/** `3 / 2` (prompt / completion), or an em dash when neither is known. */
const formatTokens = (event: AuditEvent): string =>
  event.promptTokens === null && event.completionTokens === null
    ? "—"
    : `${event.promptTokens ?? "?"} / ${event.completionTokens ?? "?"}`;

const formatDuration = (ms: number | null): string => {
  if (ms === null) {
    return "—";
  }
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
};

/**
 * Generation speed from Ollama's own timing, or an em dash for rows without
 * one (older rows, failures and non-chat kinds).
 */
const formatTokensPerSecond = (event: AuditEvent): string =>
  event.evalMs === null || event.evalMs <= 0 || event.completionTokens === null
    ? "—"
    : `${((event.completionTokens * 1000) / event.evalMs).toFixed(1)} tok/s`;

/** Local date and time, to the second. */
const formatTime = (at: string): string => {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleString();
};

const STATUS_LABEL: Record<AuditStatus, string> = {
  // The app stopped before the request ended, or it is still running.
  attempt: "no outcome",
  ok: "ok",
  error: "error",
  cancelled: "cancelled",
};

const statusLabel = (event: AuditEvent): string => {
  const label = STATUS_LABEL[event.status];
  return event.detail === null ? label : `${label} (${event.detail})`;
};

/** Kinds that read better as words; the rest are shown as they are. */
const KIND_LABEL: Partial<Record<string, string>> = {
  "ollama.memory": "Memory summary",
};

const kindLabel = (kind: string): string => KIND_LABEL[kind] ?? kind;

export {
  formatDuration,
  formatTime,
  formatTokens,
  formatTokensPerSecond,
  hostsSummary,
  kindLabel,
  statusLabel,
};
