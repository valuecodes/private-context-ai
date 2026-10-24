import { describe, expect, it } from "vitest";

import type { ModelStats } from "~/lib/types";

import { formatLoad, formatTps, sortBySpeed, speedChart } from "./model-stats";

const stats = (model: string, genTpsMedian: number): ModelStats => ({
  model,
  runs: 1,
  lastAt: "2026-10-21T08:00:00.000Z",
  genTpsMedian,
  genTpsP90: genTpsMedian,
  promptTpsMedian: null,
  loadMsMedian: null,
});

describe("sortBySpeed", () => {
  it("puts the fastest median first and breaks ties by name", () => {
    const sorted = sortBySpeed([
      stats("b", 10),
      stats("slow", 2),
      stats("a", 10),
      stats("fast", 40),
    ]);
    expect(sorted.map((entry) => entry.model)).toEqual([
      "fast",
      "a",
      "b",
      "slow",
    ]);
  });
});

describe("formatTps", () => {
  it("rounds to one decimal, or shows an em dash", () => {
    expect(formatTps(12.345)).toBe("12.3");
    expect(formatTps(null)).toBe("—");
  });
});

describe("formatLoad", () => {
  it("formats like a duration, or shows an em dash", () => {
    expect(formatLoad(250.4)).toBe("250 ms");
    expect(formatLoad(2500)).toBe("2.5 s");
    expect(formatLoad(null)).toBe("—");
  });
});

describe("speedChart", () => {
  it("is a bar per model in the order given", () => {
    expect(speedChart([stats("fast", 40.26), stats("slow", 2)])).toEqual({
      title: "Median generation speed (tok/s)",
      type: "bar",
      labels: ["fast", "slow"],
      values: [40.3, 2],
    });
  });

  it("keeps at most 100 bars", () => {
    const many = Array.from({ length: 120 }, (_, index) =>
      stats(`m${index}`, index)
    );
    const chart = speedChart(many);
    expect(chart.labels).toHaveLength(100);
    expect(chart.values).toHaveLength(100);
  });
});
