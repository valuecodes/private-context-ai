import type { LogLevel } from "./types";

const LOG_LEVELS: readonly LogLevel[] = [
  "debug",
  "info",
  "warn",
  "error",
  "silent",
];

const isLogLevel = (value: string): value is LogLevel =>
  (LOG_LEVELS as readonly string[]).includes(value);

type LevelResolution = {
  readonly level: LogLevel;
  /** Set only when a LOG_LEVEL was supplied and rejected. */
  readonly ignored?: string;
};

// `test` is silent because a test that drives real application wiring would
// otherwise interleave production log lines into the reporter output — and
// pino writes straight to fd 1, where the runner cannot capture or attribute
// them. Tests that want to assert on output build a capturing logger instead.
const defaultLevel = (nodeEnv: string | undefined): LogLevel => {
  if (nodeEnv === "production") {
    return "info";
  }
  if (nodeEnv === "test") {
    return "silent";
  }
  return "debug";
};

/**
 * An unrecognised LOG_LEVEL falls back and reports itself rather than throwing.
 * A bad log level announces itself in the very next line, so it is not worth an
 * outage — unlike config whose only symptom is silence.
 */
const resolveLevel = (env: NodeJS.ProcessEnv): LevelResolution => {
  const fallback = defaultLevel(env.NODE_ENV);
  const requested = env.LOG_LEVEL?.trim().toLowerCase();

  if (requested === undefined || requested === "") {
    return { level: fallback };
  }

  return isLogLevel(requested)
    ? { level: requested }
    : { level: fallback, ignored: requested };
};

export { isLogLevel, resolveLevel };
