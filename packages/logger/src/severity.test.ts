import { describe, expect, it } from "vitest";

import { toSeverity } from "./severity";

// The two rows that matter are `warn` and `fatal`: pino's labels and Google's
// LogSeverity names diverge there, and getting either wrong files the entry
// under DEFAULT where severity-based alerts never see it.
describe("toSeverity", () => {
  it("maps every pino level to a Google severity", () => {
    expect(toSeverity("trace")).toEqual({ severity: "DEBUG" });
    expect(toSeverity("debug")).toEqual({ severity: "DEBUG" });
    expect(toSeverity("info")).toEqual({ severity: "INFO" });
    expect(toSeverity("warn")).toEqual({ severity: "WARNING" });
    expect(toSeverity("error")).toEqual({ severity: "ERROR" });
    expect(toSeverity("fatal")).toEqual({ severity: "CRITICAL" });
  });

  it("falls back to DEFAULT for an unmapped level", () => {
    expect(toSeverity("audit")).toEqual({ severity: "DEFAULT" });
  });

  it("emits severity and nothing else, so the numeric level is dropped", () => {
    expect(Object.keys(toSeverity("info"))).toEqual(["severity"]);
  });
});
