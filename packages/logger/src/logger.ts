// pino has no `exports` map, only `main` and `browser`. Bundlers targeting
// node therefore pick `pino.js`; a bundler configured for the browser would
// silently get `browser.js`, which just console.logs.
//
// Imported as a default rather than `import { pino }`: the typings end in
// `export = pino`, so the default import is the form that carries the merged
// namespace (`destination`, `stdSerializers`, `stdTimeFunctions`) as well as
// the callable.
import pino from "pino";
import type {
  DestinationStream,
  Logger as PinoLogger,
  LoggerOptions as PinoLoggerOptions,
} from "pino";

import { withStackTrace } from "./error-reporting";
import { sanitizeBindings, sanitizeFields } from "./fields";
import { resolveLevel } from "./level";
import { toSeverity } from "./severity";
import type { LogFields, LoggerLike, LogLevel } from "./types";

type LoggerOptions = {
  /** Overrides the LOG_LEVEL / NODE_ENV resolution. */
  readonly level?: LogLevel;
  /**
   * Fields prefixed to every line, in place of pino's `{pid, hostname}`. Set a
   * service name and revision here rather than repeating them at call sites.
   */
  readonly bindings?: LogFields;
  /**
   * pino redact paths. There is no default: the right list is app-specific and
   * matchers are compiled at construction and walked per line.
   *
   * Set this in any service that handles credentials. pino's `err` serialiser
   * copies an Error's own enumerable properties, so a third-party rejection
   * (an axios error, say) carries `err.config.headers.authorization` straight
   * into the log — and passing an `Error` as `err` is exactly what this package
   * tells you to do. A reasonable starting list:
   *
   * ```ts
   * ["*.authorization", "*.token", "err.config.headers.authorization"]
   * ```
   */
  readonly redact?: PinoLoggerOptions["redact"];
  /** Test seam. Production always writes to fd 1. */
  readonly destination?: DestinationStream;
};

/**
 * Builds the pino instance whose wire format is what Cloud Logging expects.
 * Every setting here exists because the default is wrong for Cloud Run, so none
 * of them is safe to drop:
 *
 * - `messageKey: "message"` — Cloud Logging promotes `message` to the Logs
 *   Explorer summary line and leaves pino's default `msg` inert in the payload.
 * - `timestamp: isoTime` — writes `"time": "<RFC3339>"`, one of the three time
 *   forms Google recognises, which is then moved to `LogEntry.timestamp` and
 *   stripped from the payload. pino's default epoch integer matches none of
 *   them, so entries would silently carry their *collection* time instead.
 * - `base: null` unless bindings are given — on Cloud Run `pid` is always 1 and
 *   `hostname` is a throwaway sandbox id already exposed as `labels.instanceId`.
 * - `formatters.level` — see severity.ts.
 * - `formatters.log` — see error-reporting.ts.
 *
 * Everything, including ERROR, goes to **stdout**. Cloud Run tags anything on
 * stderr as ERROR automatically, which would fight the explicit severity above.
 *
 * `sync: true` is not pino's default, and the default is the wrong choice here:
 * async writes rely on an exit-time flush, but a serverless runtime throttles
 * CPU to near zero once a response is sent — exactly when a background task
 * writes the ERROR explaining a failure — and an app may call `process.exit()`
 * on SIGTERM. Writing synchronously means there is never anything to flush.
 */
const buildPino = (options: LoggerOptions): PinoLogger => {
  const resolution =
    options.level === undefined
      ? resolveLevel(process.env)
      : { level: options.level, ignored: undefined };

  const instance = pino(
    {
      level: resolution.level,
      messageKey: "message",
      errorKey: "err",
      timestamp: pino.stdTimeFunctions.isoTime,
      base:
        options.bindings === undefined
          ? null
          : sanitizeBindings(options.bindings),
      formatters: { level: toSeverity, log: withStackTrace },
      // Omitted rather than passed as undefined: pino validates the shape of
      // `redact` when the key is present.
      ...(options.redact === undefined ? {} : { redact: options.redact }),
    },
    options.destination ?? pino.destination({ dest: 1, sync: true })
  );

  if (resolution.ignored !== undefined) {
    instance.warn(
      { ignored: resolution.ignored, level: resolution.level },
      "LOG_LEVEL not recognised, falling back"
    );
  }

  return instance;
};

/**
 * A logger that emits Cloud Logging shaped single-line JSON on stdout.
 *
 * ```ts
 * const logger = new Logger({ bindings: { service: "api" } });
 * logger.info("server ready", { port: 8080 });
 * logger.error("request failed", { err, requestId: "abc" });
 * ```
 *
 * Message first, fields second, in every method — see `LogMethod`. Passing an
 * `Error` as the `err` field is what produces the `stack_trace` that Cloud
 * Error Reporting groups on.
 */
class Logger implements LoggerLike {
  private readonly delegate: PinoLogger;

  /**
   * `delegate` is an internal seam for `child()` only: re-running the option
   * path there would open a second stream on fd 1. Callers pass options.
   */
  constructor(options: LoggerOptions = {}, delegate?: PinoLogger) {
    this.delegate = delegate ?? buildPino(options);
  }

  debug(message: string, fields?: LogFields): void {
    this.write("debug", message, fields);
  }

  info(message: string, fields?: LogFields): void {
    this.write("info", message, fields);
  }

  warn(message: string, fields?: LogFields): void {
    this.write("warn", message, fields);
  }

  error(message: string, fields?: LogFields): void {
    this.write("error", message, fields);
  }

  /** A logger that prefixes `bindings` to every line it emits. */
  child(bindings: LogFields): Logger {
    return new Logger({}, this.delegate.child(sanitizeBindings(bindings)));
  }

  // The single place the argument order is flipped back to pino's own. Keeping
  // it in one method is the reason the public shape cannot drift per level.
  private write(
    level: "debug" | "info" | "warn" | "error",
    message: string,
    fields: LogFields | undefined
  ): void {
    if (fields === undefined) {
      this.delegate[level](message);
      return;
    }
    this.delegate[level](sanitizeFields(fields), message);
  }
}

// No module-level singleton on purpose. Building one would open a stream on
// fd 1 as an import side effect for every consumer, including those that go on
// to construct their own with bindings — which is what an app should do, in one
// place it owns.
export type { LoggerOptions };
export { Logger };
