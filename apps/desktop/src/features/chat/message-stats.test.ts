import { describe, expect, it } from "vitest";

import { formatStats } from "./message-stats";

describe("formatStats", () => {
  it("reads tokens per second from the generation time", () => {
    expect(
      formatStats({ completionTokens: 100, durationMs: 2500, evalMs: 2000 })
    ).toBe("100 tokens · 50 tok/s · 2.5s");
  });

  it("rounds the rate to one decimal", () => {
    expect(
      formatStats({ completionTokens: 37, durationMs: 3000, evalMs: 3000 })
    ).toBe("37 tokens · 12.3 tok/s · 3.0s");
  });

  it("estimates from the whole duration without evalMs", () => {
    expect(
      formatStats({ completionTokens: 100, durationMs: 4000, evalMs: null })
    ).toBe("100 tokens · ≈25 tok/s · 4.0s");
  });

  it("leaves out what is not known", () => {
    expect(
      formatStats({ completionTokens: 1, durationMs: null, evalMs: null })
    ).toBe("1 token");
    expect(formatStats({ completionTokens: 5, durationMs: 0, evalMs: 0 })).toBe(
      "5 tokens · 0.0s"
    );
    expect(
      formatStats({ completionTokens: null, durationMs: 10, evalMs: 5 })
    ).toBeNull();
  });
});
