import { describe, expect, it } from "vitest";

import { sanitizeBindings, sanitizeFields } from "./fields";

describe("sanitizeFields", () => {
  // pino resolves `serializers[key]` / `stringifiers[key]` with plain bracket
  // access, so these keys hit Object.prototype and either throw or emit an
  // unparseable line. See fields.ts.
  it("drops keys inherited from Object.prototype", () => {
    const fields = JSON.parse(
      '{"hasOwnProperty":"x","toString":"y","constructor":"z","__proto__":"w","ok":1}'
    ) as Record<string, unknown>;

    expect(sanitizeFields(fields)).toEqual({ ok: 1 });
  });

  it("drops the Cloud Logging keys a call site must not forge", () => {
    expect(
      sanitizeFields({
        severity: "DEBUG",
        time: "1999-01-01T00:00:00.000Z",
        message: "spoofed",
        stack_trace: "fake",
        "logging.googleapis.com/trace": "attacker",
        "logging.googleapis.com/spanId": "attacker",
        "logging.googleapis.com/trace_sampled": true,
        requestId: "a",
      })
    ).toEqual({ requestId: "a" });
  });

  // insertId is the deduplication key, labels/operation drive filtering and
  // request grouping, httpRequest is promoted whole.
  it("drops the other promoted Cloud Logging metadata fields", () => {
    expect(
      sanitizeFields({
        httpRequest: { requestMethod: "DELETE" },
        timestamp: "1999-01-01T00:00:00.000Z",
        timestampSeconds: 0,
        timestampNanos: 0,
        "logging.googleapis.com/insertId": "dedupe-me",
        "logging.googleapis.com/labels": { env: "spoofed" },
        "logging.googleapis.com/operation": { id: "spoofed" },
        "logging.googleapis.com/sourceLocation": { file: "spoofed" },
        requestId: "a",
      })
    ).toEqual({ requestId: "a" });
  });

  it("leaves an ordinary field object untouched", () => {
    const fields = { requestId: "a", durationMs: 3, err: new Error("x") };

    // Same reference: the no-op path must not allocate.
    expect(sanitizeFields(fields)).toBe(fields);
  });
});

describe("sanitizeBindings", () => {
  // child(parseTraceHeaders(...)) is the supported way to attach trace fields,
  // so bindings keep the reserved keys that call-site fields lose.
  it("keeps reserved keys, which bindings are allowed to set", () => {
    const bindings = {
      "logging.googleapis.com/trace": "abc",
      service: "api",
    };

    expect(sanitizeBindings(bindings)).toBe(bindings);
  });

  it("still drops keys inherited from Object.prototype", () => {
    const bindings = JSON.parse(
      '{"hasOwnProperty":"x","service":"api"}'
    ) as Record<string, unknown>;

    expect(sanitizeBindings(bindings)).toEqual({ service: "api" });
  });
});
