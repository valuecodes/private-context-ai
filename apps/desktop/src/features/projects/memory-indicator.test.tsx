import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { MemoryIndicator } from "./memory-indicator";
import type { ProjectMemoryStatus } from "./use-memory-status";

const indicator = (state: ProjectMemoryStatus["state"], pending?: number) =>
  renderToString(
    <MemoryIndicator state={state} pending={pending} onOpenActivity={vi.fn()} />
  );

describe("MemoryIndicator", () => {
  it("shows nothing when quiet", () => {
    expect(indicator(null)).toBe("");
    expect(indicator(null, 0)).toBe("");
  });

  it("pulses while a summary is written", () => {
    const html = indicator("running");
    expect(html).toContain('role="status"');
    expect(html).toContain('aria-label="Saving to project memory…"');
    expect(html).toContain("animate-pulse");
  });

  it("says when memory was updated", () => {
    const html = indicator("saved");
    expect(html).toContain('aria-label="Memory updated"');
    expect(html).not.toContain("animate-pulse");
  });

  it("offers Activity when memory could not be updated", () => {
    const html = indicator("failed");
    expect(html).toMatch(
      /<button[^>]*aria-label="Couldn&#x27;t update memory\. Open Activity"/
    );
  });

  it("counts pending memories", () => {
    expect(indicator(null, 1)).toContain('aria-label="1 memory pending"');
    const html = indicator("running", 2);
    expect(html).toContain('aria-label="2 memories pending"');
    expect(html).toContain(">2</span>");
  });
});
