/** A finished reply's counts, from Ollama's last stream line. */
type ReplyStats = {
  completionTokens: number | null;
  /** The whole request, model load included. */
  durationMs: number | null;
  /** Generating the completion tokens only; null for older replies. */
  evalMs: number | null;
};

/** One decimal, without a trailing `.0`. */
const oneDecimal = (value: number): string =>
  String(Math.round(value * 10) / 10);

/**
 * "N tokens · X tok/s · Ys". Without `evalMs` (older replies) the rate is an
 * estimate over the whole duration and starts with "≈". `null` when the
 * token count is not known.
 */
const formatStats = ({
  completionTokens,
  durationMs,
  evalMs,
}: ReplyStats): string | null => {
  if (completionTokens === null) {
    return null;
  }
  const parts = [
    `${completionTokens} ${completionTokens === 1 ? "token" : "tokens"}`,
  ];
  if (evalMs !== null && evalMs > 0) {
    parts.push(`${oneDecimal((completionTokens * 1000) / evalMs)} tok/s`);
  } else if (durationMs !== null && durationMs > 0) {
    parts.push(`≈${oneDecimal((completionTokens * 1000) / durationMs)} tok/s`);
  }
  if (durationMs !== null) {
    parts.push(`${(durationMs / 1000).toFixed(1)}s`);
  }
  return parts.join(" · ");
};

export type { ReplyStats };
export { formatStats };
