import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { ModelStats } from "~/lib/types";

import { ModelStatsView, ModelsPanel } from "./models-panel";

vi.mock("~/lib/ipc");

const stats = (
  model: string,
  overrides: Partial<ModelStats> = {}
): ModelStats => ({
  model,
  runs: 3,
  lastAt: "2026-10-21T08:00:00.000Z",
  genTpsMedian: 20,
  genTpsP90: 30,
  promptTpsMedian: null,
  loadMsMedian: null,
  ...overrides,
});

describe("ModelStatsView", () => {
  it("says when there is no data yet", () => {
    const html = renderToString(<ModelStatsView stats={[]} />);
    expect(html).toContain("No speed data yet");
    expect(html).not.toContain("<table");
  });

  it("shows the chart and a row per model, fastest first", () => {
    const html = renderToString(
      <ModelStatsView
        stats={[
          stats("slow-model", { genTpsMedian: 4.25, genTpsP90: 6 }),
          stats("fast-model", {
            genTpsMedian: 55,
            genTpsP90: 61.5,
            promptTpsMedian: 410,
            loadMsMedian: 1500,
            runs: 12,
          }),
        ]}
      />
    );
    expect(html).toContain("Median generation speed (tok/s)");
    expect(html).toMatch(/<svg[^>]*role="img"/);
    expect(html.indexOf("fast-model")).toBeLessThan(html.indexOf("slow-model"));
    for (const text of ["Requests", "55.0", "61.5", "410.0", "1.5 s", "12"]) {
      expect(html).toContain(text);
    }
    // A model without prompt or load timings shows em dashes.
    expect(html).toMatch(/4\.3<\/td><td[^>]*>6\.0<\/td><td[^>]*>—/);
  });
});

describe("ModelsPanel", () => {
  it("renders its heading and explains where the numbers come from", () => {
    const html = renderToString(<ModelsPanel />);
    expect(html).toContain("Models");
    expect(html).toContain("Refresh");
    expect(html).toContain("retained Activity log");
  });
});
