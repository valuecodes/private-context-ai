import { Logger } from "./logger";
import type { LogLevel } from "./types";

type LogLine = Record<string, unknown>;

type TestLogger = {
  readonly logger: Logger;
  /** Every line emitted so far, parsed back from the serialised JSON. */
  readonly lines: readonly LogLine[];
};

/**
 * Captures the **serialised** line and parses it back, so assertions are about
 * what Cloud Logging will actually receive rather than about what was handed to
 * the logger. That is the whole point: the interesting failures in this package
 * (`msg` instead of `message`, a numeric `level` instead of `severity`, a
 * missing `stack_trace`) are all invisible before serialisation.
 *
 * `debug` by default so a capturing logger sees everything regardless of the
 * ambient NODE_ENV, which resolves to `silent` under a test runner.
 */
const createTestLogger = (level: LogLevel = "debug"): TestLogger => {
  const lines: LogLine[] = [];

  const logger = new Logger({
    level,
    destination: {
      write: (line: string) => {
        lines.push(JSON.parse(line) as LogLine);
      },
    },
  });

  return { logger, lines };
};

export type { LogLine, TestLogger };
export { createTestLogger };
