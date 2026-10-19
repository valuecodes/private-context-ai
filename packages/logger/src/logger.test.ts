import { describe, expect, it } from "vitest";

import { Logger } from "./logger";
import { createTestLogger } from "./testing";

const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

// This file pins the wire format. Everything asserted here is something Cloud
// Logging reads by name, and every one of them fails silently in production:
// a wrong key is not an error, it is just a field nobody looks at.
describe("Logger", () => {
  it("emits one Cloud Logging shaped line per call", () => {
    const { logger, lines } = createTestLogger();

    logger.info("server tick", { due: 2 });

    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatchObject({
      severity: "INFO",
      message: "server tick",
      due: 2,
    });
  });

  it("logs a bare message with no fields", () => {
    const { logger, lines } = createTestLogger();

    logger.info("server ready");

    expect(lines[0]).toMatchObject({
      severity: "INFO",
      message: "server ready",
    });
  });

  it("drops the pino defaults Cloud Logging cannot use", () => {
    const { logger, lines } = createTestLogger();

    logger.info("ready");

    const line = lines[0] ?? {};
    // A numeric `level` here would mean formatters.level is missing and every
    // entry lands as DEFAULT severity.
    expect(line).not.toHaveProperty("level");
    // `msg` is pino's default and is not promoted by Cloud Logging.
    expect(line).not.toHaveProperty("msg");
    // pid is always 1 on Cloud Run and hostname is already labels.instanceId.
    expect(line).not.toHaveProperty("pid");
    expect(line).not.toHaveProperty("hostname");
  });

  it("writes the timestamp as an RFC3339 string under `time`", () => {
    const { logger, lines } = createTestLogger();

    logger.info("ready");

    expect(lines[0]?.time).toMatch(ISO);
  });

  it("maps each level to its Google severity end to end", () => {
    const { logger, lines } = createTestLogger();

    logger.debug("d");
    logger.info("i");
    logger.warn("w");
    logger.error("e");

    expect(lines.map((line) => line.severity)).toEqual([
      "DEBUG",
      "INFO",
      "WARNING",
      "ERROR",
    ]);
  });

  it("prefixes base bindings to every line", () => {
    const lines: Record<string, unknown>[] = [];
    const logger = new Logger({
      level: "debug",
      bindings: { service: "api", revision: "rev-1" },
      destination: {
        write: (line: string) => {
          lines.push(JSON.parse(line) as Record<string, unknown>);
        },
      },
    });

    logger.info("a");
    logger.warn("b");

    expect(lines).toHaveLength(2);
    for (const line of lines) {
      expect(line).toMatchObject({ service: "api", revision: "rev-1" });
    }
  });

  it("carries child bindings onto every subsequent line", () => {
    const { logger, lines } = createTestLogger();
    const child = logger.child({
      requestId: "abc",
      at: "2026-09-05T12:00:00.000Z",
    });

    child.info("handling");
    child.child({ attempt: 2 }).warn("retrying");

    expect(lines[0]).toMatchObject({
      requestId: "abc",
      at: "2026-09-05T12:00:00.000Z",
      message: "handling",
    });
    expect(lines[1]).toMatchObject({ requestId: "abc", attempt: 2 });
  });

  it("shares one destination between a parent and its children", () => {
    const { logger, lines } = createTestLogger();

    logger.info("parent");
    logger.child({ a: 1 }).info("child");

    // A child that rebuilt its own destination would write to fd 1 instead,
    // and this would be 1.
    expect(lines).toHaveLength(2);
  });

  it("attaches an Error as `err` plus a top-level stack_trace", () => {
    const { logger, lines } = createTestLogger();

    logger.error("request failed", {
      err: new Error("boom"),
      requestId: "a",
    });

    expect(lines[0]).toMatchObject({
      severity: "ERROR",
      message: "request failed",
      requestId: "a",
      err: { type: "Error", message: "boom" },
    });
    expect(lines[0]?.stack_trace).toContain("Error: boom");
  });

  // Trace fields are ordinary child bindings: the app builds one traced child
  // per request and passes it down, so an untraced logger stays untraced.
  it("carries trace fields only on the traced child", () => {
    const { logger, lines } = createTestLogger();

    const traced = logger.child({
      "logging.googleapis.com/trace": "abc",
      "logging.googleapis.com/spanId": "def",
    });

    traced.info("inside");
    logger.info("outside");

    expect(lines[0]).toMatchObject({ "logging.googleapis.com/trace": "abc" });
    expect(lines[1]).not.toHaveProperty("logging.googleapis.com/trace");
  });

  // Bindings are re-serialised per line rather than accumulated, so one call's
  // fields cannot bleed into the next line from the same logger.
  it("does not leak one line's fields onto the next", () => {
    const { logger, lines } = createTestLogger();
    const traced = logger.child({ "logging.googleapis.com/trace": "abc" });

    traced.warn("first", { requestedAt: "then" });
    traced.info("second", { due: 1 });

    expect(lines[1]).toMatchObject({
      message: "second",
      due: 1,
      "logging.googleapis.com/trace": "abc",
    });
    expect(lines[1]).not.toHaveProperty("requestedAt");
  });

  // Bindings are written ahead of call-site fields and nothing de-duplicates
  // them, so a collision emits the key twice and the last one — the call site's
  // — is what a JSON parser and Cloud Logging keep.
  it("lets a call-site field win over a binding of the same name", () => {
    const { logger, lines } = createTestLogger();
    const scoped = logger.child({ region: "ambient" });

    scoped.info("override", { region: "explicit" });

    expect(lines[0]?.region).toBe("explicit");
  });

  // ...but NOT for the keys Cloud Logging reads by name. Without this, an
  // object spread from untrusted input could file an ERROR as DEBUG, where no
  // `severity >= ERROR` alert would ever see it.
  it("refuses to let a call-site field forge severity, time or the trace", () => {
    const { logger, lines } = createTestLogger();
    const traced = logger.child({
      "logging.googleapis.com/trace": "ambient",
    });

    traced.error("auth bypass attempt", {
      severity: "DEBUG",
      time: "1999-01-01T00:00:00.000Z",
      "logging.googleapis.com/trace": "attacker",
      requestId: "a",
    });

    expect(lines[0]).toMatchObject({
      severity: "ERROR",
      "logging.googleapis.com/trace": "ambient",
      requestId: "a",
    });
    expect(lines[0]?.time).not.toBe("1999-01-01T00:00:00.000Z");
  });

  // pino resolves its per-key handlers with bracket access, so these keys would
  // otherwise throw (`hasOwnProperty`) or emit an unparseable line (`toString`).
  it("survives field names that collide with Object.prototype", () => {
    const { logger, lines } = createTestLogger();

    expect(() => {
      logger.info(
        "req",
        JSON.parse('{"hasOwnProperty":"x","toString":"y","ok":1}') as Record<
          string,
          unknown
        >
      );
    }).not.toThrow();

    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatchObject({ message: "req", ok: 1 });
    expect(lines[0]).not.toHaveProperty("hasOwnProperty", "x");
  });

  // pino's err serialiser copies an Error's own properties, so a third-party
  // rejection can carry an Authorization header into the log. LoggerOptions
  // must expose redact, or the README's mitigation is impossible to apply.
  it("forwards redact paths to pino", () => {
    const lines: Record<string, unknown>[] = [];
    const logger = new Logger({
      level: "debug",
      redact: ["err.config.headers.authorization"],
      destination: {
        write: (line: string) => {
          lines.push(JSON.parse(line) as Record<string, unknown>);
        },
      },
    });

    logger.error("http call failed", {
      err: Object.assign(new Error("401"), {
        config: { headers: { authorization: "Bearer SUPER_SECRET" } },
      }),
    });

    expect(JSON.stringify(lines[0])).not.toContain("SUPER_SECRET");
  });

  it("emits nothing at all when silent", () => {
    const { logger, lines } = createTestLogger("silent");

    logger.error("ignored", { err: new Error("boom") });

    expect(lines).toHaveLength(0);
  });

  it("respects the level threshold", () => {
    const { logger, lines } = createTestLogger("warn");

    logger.info("dropped");
    logger.warn("kept");

    expect(lines.map((line) => line.message)).toEqual(["kept"]);
  });

  // A logger that discarded EVERY field when one was circular would fail
  // precisely when the fields matter most. pino degrades per field.
  it("degrades only the unserialisable field", () => {
    const { logger, lines } = createTestLogger();
    const circular: Record<string, unknown> = { name: "loop" };
    circular.self = circular;

    logger.info("still logged", { circular, requestId: "a" });

    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatchObject({ requestId: "a", message: "still logged" });
  });

  // The message is never treated as a format string, so a value that looks like
  // a pino placeholder cannot consume the fields.
  it("does not interpolate fields into the message", () => {
    const { logger, lines } = createTestLogger();

    logger.info("progress %s of %d", { done: 3, total: 10 });

    expect(lines[0]).toMatchObject({
      message: "progress %s of %d",
      done: 3,
      total: 10,
    });
  });
});
