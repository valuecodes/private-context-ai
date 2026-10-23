import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { ChartSpec } from "~/lib/types";

import { ChartBlock, ChartFailed, ChartTable } from "./chart-block";

vi.mock("~/lib/ipc");

const spec = (
  type: ChartSpec["type"],
  values: number[],
  labels = values.map((_, index) => `L${index}`)
): ChartSpec => ({ title: "Population", type, labels, values });

/** Matches inside the chart's own SVG, not the buttons' icons. */
const count = (html: string, pattern: RegExp): number => {
  const chart = /<svg[^>]*role="img"[\s\S]*?<\/svg>/.exec(html)?.[0] ?? "";
  return chart.match(pattern)?.length ?? 0;
};

/** Every `x`, `y`, `cx`, `cy`, `width`, `height`, `points` and `d` value. */
const geometry = (html: string): string[] =>
  [
    ...html.matchAll(
      / (?:x|y|cx|cy|x1|x2|y1|y2|width|height|points|d)="([^"]*)"/g
    ),
  ].map((match) => match[1] ?? "");

describe("ChartBlock", () => {
  it("draws a bar per value with a title each", () => {
    const html = renderToString(<ChartBlock spec={spec("bar", [4, -2, 6])} />);
    expect(count(html, /<rect /g)).toBe(3);
    expect(html).toContain("<title>L1: -2</title>");
    expect(html).toMatch(/<svg[^>]*role="img"[^>]*aria-labelledby="/);
    expect(html).toContain("Population</figcaption>");
    expect(html).toContain('aria-pressed="false"');
  });

  it("draws a line through a point per value", () => {
    const html = renderToString(
      <ChartBlock spec={spec("line", [1, 2, 3, 4])} />
    );
    expect(count(html, /<polyline /g)).toBe(1);
    expect(count(html, /<circle /g)).toBe(4);
    expect(count(html, /<rect /g)).toBe(0);
  });

  it("draws a pie slice per non-zero value, and a legend", () => {
    const html = renderToString(
      <ChartBlock spec={spec("pie", [1, 0, 2, 3])} />
    );
    expect(count(html, /<path /g)).toBe(3);
    expect(html.match(/<li /g)).toHaveLength(4);
    // A single slice is a whole circle: an arc cannot draw one.
    const single = renderToString(<ChartBlock spec={spec("pie", [0, 5])} />);
    expect(count(single, /<circle /g)).toBe(1);
    expect(count(single, /<path /g)).toBe(0);
  });

  it("keeps coordinates finite for flat and extreme data", () => {
    for (const chart of [
      spec("bar", [0, 0]),
      spec("line", [7, 7, 7]),
      spec("line", [5]),
      spec("bar", [1e15, -1e15]),
    ]) {
      const values = geometry(renderToString(<ChartBlock spec={chart} />));
      expect(values.length).toBeGreaterThan(0);
      for (const value of values) {
        expect(value).not.toMatch(/NaN|Infinity/);
      }
    }
  });

  it("shows model text only as escaped text", () => {
    const evil = spec("bar", [1], ['<script>alert(1)</script><a href="x">']);
    const html = renderToString(
      <ChartBlock spec={{ ...evil, title: '<img src="x" onerror="y">' }} />
    );
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;img");
    // Escaped text has no `<`, so this only looks inside real tags.
    expect(html).not.toMatch(/<[^>]*\s(?:href|style|onerror)=/);
    expect(html).not.toMatch(/<foreignObject|<image|<use/);
    expect(html).not.toContain("dangerouslySetInnerHTML");
  });
});

describe("ChartTable", () => {
  it("lists every label and value", () => {
    const html = renderToString(
      <ChartTable spec={spec("pie", [1.5, 2], ["a", "b"])} />
    );
    expect(html.match(/<tr /g)).toHaveLength(3);
    expect(html).toContain('<caption class="sr-only">Population</caption>');
    expect(html).toMatch(/<td[^>]*>a<\/td><td[^>]*>1.5<\/td>/);
  });
});

describe("ChartFailed", () => {
  it("says the chart could not be shown, with why as a tooltip", () => {
    const html = renderToString(
      <ChartFailed reason="a pie has at most 12 slices" />
    );
    expect(html).toContain("Chart could not be shown");
    expect(html).toContain('title="a pie has at most 12 slices"');
  });
});
