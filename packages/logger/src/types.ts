type LogFields = Readonly<Record<string, unknown>>;

/**
 * Message first, fields second — the opposite of pino's own `(obj, msg)`.
 *
 * The order is not cosmetic. `Logger` wraps pino rather than re-exporting it,
 * so pino's `(message, ...args)` printf overload is unreachable from here: a
 * call cannot accidentally land on it and interpolate the fields *into* the
 * message string, which is the one silent way to lose a log line's contents.
 * The wrapper owns the shape, so there is only one shape.
 */
type LogMethod = (message: string, fields?: LogFields) => void;

/**
 * The structural contract to depend on. `Logger` implements it, and because it
 * names no private state a test double can be a plain object literal.
 */
type LoggerLike = {
  readonly debug: LogMethod;
  readonly info: LogMethod;
  readonly warn: LogMethod;
  readonly error: LogMethod;
  /**
   * Bindings are serialised once here, not per line, and are written *before*
   * call-site fields. Re-adding a bound key at a call site therefore emits the
   * key twice rather than overwriting it.
   */
  readonly child: (bindings: LogFields) => LoggerLike;
};

/**
 * The configurable threshold. Matches the methods above, so pino's `trace` and
 * `fatal` are deliberately not part of the vocabulary — `severity.ts` still
 * maps them, because pino renders a severity for every level it knows about.
 */
type LogLevel = "debug" | "info" | "warn" | "error" | "silent";

export type { LogFields, LoggerLike, LogLevel, LogMethod };
