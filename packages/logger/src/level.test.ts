import { describe, expect, it } from "vitest";

import { isLogLevel, resolveLevel } from "./level";

describe("resolveLevel", () => {
  it("prefers a valid LOG_LEVEL over the NODE_ENV default", () => {
    expect(
      resolveLevel({ NODE_ENV: "production", LOG_LEVEL: "debug" })
    ).toEqual({ level: "debug" });
  });

  it("defaults by NODE_ENV when LOG_LEVEL is absent", () => {
    expect(resolveLevel({ NODE_ENV: "production" })).toEqual({ level: "info" });
    expect(resolveLevel({ NODE_ENV: "test" })).toEqual({ level: "silent" });
    expect(resolveLevel({})).toEqual({ level: "debug" });
  });

  it("ignores whitespace and case", () => {
    expect(resolveLevel({ LOG_LEVEL: "  WARN " })).toEqual({ level: "warn" });
  });

  it("treats an empty LOG_LEVEL as unset", () => {
    expect(resolveLevel({ NODE_ENV: "production", LOG_LEVEL: "" })).toEqual({
      level: "info",
    });
  });

  // Reporting rather than throwing: a bad level announces itself in the next
  // line, so it is not worth taking the process down for.
  it("falls back and reports an unrecognised LOG_LEVEL", () => {
    expect(
      resolveLevel({ NODE_ENV: "production", LOG_LEVEL: "verbose" })
    ).toEqual({ level: "info", ignored: "verbose" });
  });

  it("rejects pino levels outside this package's vocabulary", () => {
    expect(isLogLevel("trace")).toBe(false);
    expect(isLogLevel("fatal")).toBe(false);
    expect(isLogLevel("silent")).toBe(true);
  });
});
