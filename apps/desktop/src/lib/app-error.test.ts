import { describe, expect, it } from "vitest";

import { toAppError } from "./app-error";

describe("toAppError", () => {
  it("passes an AppError through", () => {
    const error = { kind: "busy", message: "still answering" };
    expect(toAppError(error)).toBe(error);
  });

  it("wraps a plain string rejection as internal", () => {
    expect(toAppError("invalid args")).toEqual({
      kind: "internal",
      message: "invalid args",
    });
  });

  it("treats unknown shapes as internal", () => {
    expect(toAppError({ kind: "nope", message: "x" })).toEqual({
      kind: "internal",
      message: "unexpected error",
    });
    expect(toAppError(undefined)).toEqual({
      kind: "internal",
      message: "unexpected error",
    });
  });
});
