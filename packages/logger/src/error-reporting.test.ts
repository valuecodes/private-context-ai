import { describe, expect, it } from "vitest";

import { withStackTrace } from "./error-reporting";

const stackOf = (object: Record<string, unknown>): string => {
  const trace = object.stack_trace;
  return typeof trace === "string" ? trace : "";
};

describe("withStackTrace", () => {
  it("promotes an Error's stack to a top-level stack_trace", () => {
    const result = withStackTrace({ err: new Error("boom"), taskId: "a" });

    expect(stackOf(result)).toContain("Error: boom");
    expect(stackOf(result)).toContain("    at ");
    // The rest of the line must survive untouched.
    expect(result.taskId).toBe("a");
  });

  // The reason the std serialiser is used instead of reading `error.stack`:
  // Error Reporting groups on these frames, and a wrapper's own stack would
  // group every distinct root cause together.
  it("folds a cause chain into the stack_trace", () => {
    const result = withStackTrace({
      err: new Error("fetch failed", { cause: new Error("ECONNREFUSED") }),
    });

    expect(stackOf(result)).toContain("Error: fetch failed");
    expect(stackOf(result)).toContain("ECONNREFUSED");
  });

  it("leaves the object alone when err is not an Error", () => {
    const line = { err: "just a string", message: "x" };

    expect(withStackTrace(line)).toEqual(line);
  });

  it("leaves the object alone when there is no err at all", () => {
    const line = { taskId: "a", durationMs: 3 };

    expect(withStackTrace(line)).toEqual(line);
  });
});
