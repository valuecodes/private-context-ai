import { describe, expect, it } from "vitest";

import type { ModelCapabilities, ThinkStyle } from "~/lib/types";

import { selectedLevel, THINK_OFF } from "./think-choice";

const caps = (
  thinking: boolean,
  thinkStyle: ThinkStyle = { kind: "bool" }
): ModelCapabilities => ({
  thinking,
  vision: false,
  tools: false,
  embeddingOnly: false,
  thinkStyle,
});

const levels = (canDisable: boolean): ThinkStyle => ({
  kind: "levels",
  levels: ["low", "medium", "high"],
  default: "medium",
  canDisable,
});

describe("selectedLevel", () => {
  it("shows no level for boolean models", () => {
    expect(selectedLevel(caps(true), { kind: "on" })).toBeNull();
  });

  it("shows only advertised levels", () => {
    const always = caps(true, levels(false));
    expect(selectedLevel(always, { kind: "level", level: "high" })).toBe(
      "high"
    );
    expect(selectedLevel(always, { kind: "on" })).toBe("medium");
    const narrow = caps(true, {
      kind: "levels",
      levels: ["low"],
      default: "low",
      canDisable: false,
    });
    expect(selectedLevel(narrow, { kind: "level", level: "high" })).toBe("low");
  });

  it("shows Off for a levels model only when it allows it", () => {
    expect(selectedLevel(caps(true, levels(true)), THINK_OFF)).toBeNull();
    expect(selectedLevel(caps(true, levels(false)), THINK_OFF)).toBe("medium");
  });

  it("passes model-defined levels through exactly", () => {
    const custom = caps(true, {
      kind: "levels",
      levels: ["low", "xhigh", "max"],
      default: "max",
      canDisable: false,
    });
    expect(selectedLevel(custom, { kind: "level", level: "xhigh" })).toBe(
      "xhigh"
    );
    expect(selectedLevel(custom, THINK_OFF)).toBe("max");
  });
});
