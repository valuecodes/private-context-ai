import { describe, expect, it } from "vitest";

import { parseTraceHeaders } from "./trace";

const TRACE = "4bf92f3577b34da6a3ce929d0e0e4736";

describe("parseTraceHeaders", () => {
  it("parses a W3C traceparent", () => {
    expect(
      parseTraceHeaders({ traceparent: `00-${TRACE}-00f067aa0ba902b7-01` })
    ).toEqual({
      "logging.googleapis.com/trace": TRACE,
      "logging.googleapis.com/spanId": "00f067aa0ba902b7",
      "logging.googleapis.com/trace_sampled": true,
    });
  });

  it("reads sampled from the low bit of the traceparent flags", () => {
    const parsed = parseTraceHeaders({
      traceparent: `00-${TRACE}-00f067aa0ba902b7-00`,
    });

    expect(parsed?.["logging.googleapis.com/trace_sampled"]).toBe(false);
  });

  // X-Cloud-Trace-Context carries the span as a decimal uint64, but
  // logging.googleapis.com/spanId wants the 16-char hex Trace v2 uses. Skipping
  // this conversion is silent — the field is accepted and simply never matches.
  it("converts the Cloud Trace decimal span to 16-char hex", () => {
    expect(
      parseTraceHeaders({ cloudTraceContext: `${TRACE}/1234567890;o=1` })
    ).toEqual({
      "logging.googleapis.com/trace": TRACE,
      "logging.googleapis.com/spanId": "00000000499602d2",
      "logging.googleapis.com/trace_sampled": true,
    });
  });

  it("handles a Cloud Trace header with no span and no sampling flag", () => {
    expect(parseTraceHeaders({ cloudTraceContext: TRACE })).toEqual({
      "logging.googleapis.com/trace": TRACE,
      "logging.googleapis.com/spanId": undefined,
      "logging.googleapis.com/trace_sampled": false,
    });
  });

  it("prefers traceparent when both headers are present", () => {
    const parsed = parseTraceHeaders({
      traceparent: `00-${TRACE}-00f067aa0ba902b7-01`,
      cloudTraceContext: `${"a".repeat(32)}/99;o=1`,
    });

    expect(parsed?.["logging.googleapis.com/trace"]).toBe(TRACE);
  });

  it("falls back to Cloud Trace when the traceparent is malformed", () => {
    const parsed = parseTraceHeaders({
      traceparent: "garbage",
      cloudTraceContext: `${TRACE}/99`,
    });

    expect(parsed?.["logging.googleapis.com/trace"]).toBe(TRACE);
  });

  it("returns undefined when there is nothing usable", () => {
    expect(parseTraceHeaders({})).toBeUndefined();
    expect(
      parseTraceHeaders({ traceparent: "00-short-00f067aa0ba902b7-01" })
    ).toBeUndefined();
    expect(
      parseTraceHeaders({ cloudTraceContext: "not-a-trace" })
    ).toBeUndefined();
    // An all-zero id is the "no trace" sentinel, not a trace.
    expect(
      parseTraceHeaders({ cloudTraceContext: "0".repeat(32) })
    ).toBeUndefined();
  });

  // Google documents `logging.googleapis.com/trace` as the qualified resource
  // name; the bare id is only the default because it needs no project id.
  it("qualifies the trace with a project id when one is given", () => {
    expect(
      parseTraceHeaders(
        { traceparent: `00-${TRACE}-00f067aa0ba902b7-01` },
        { projectId: "my-project" }
      )
    ).toMatchObject({
      "logging.googleapis.com/trace": `projects/my-project/traces/${TRACE}`,
      "logging.googleapis.com/spanId": "00f067aa0ba902b7",
    });
  });

  it("qualifies the Cloud Trace path too", () => {
    expect(
      parseTraceHeaders(
        { cloudTraceContext: `${TRACE}/1234567890;o=1` },
        { projectId: "my-project" }
      )
    ).toMatchObject({
      "logging.googleapis.com/trace": `projects/my-project/traces/${TRACE}`,
    });
  });

  it("leaves the id bare when no project id is given", () => {
    expect(
      parseTraceHeaders({ traceparent: `00-${TRACE}-00f067aa0ba902b7-01` }, {})
    ).toMatchObject({ "logging.googleapis.com/trace": TRACE });
  });

  it("drops a span that cannot be a uint64", () => {
    const parsed = parseTraceHeaders({
      cloudTraceContext: `${TRACE}/99999999999999999999`,
    });

    expect(parsed?.["logging.googleapis.com/spanId"]).toBeUndefined();
    expect(parsed?.["logging.googleapis.com/trace"]).toBe(TRACE);
  });
});
