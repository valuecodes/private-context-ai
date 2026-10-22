import { describe, expect, it } from "vitest";

import type { ModelCapabilities, ThinkStyle } from "~/lib/types";

import { selectedLevel, THINK_OFF, thinkToSend } from "./think-choice";

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

describe("thinkToSend", () => {
  it("sends nothing for unknown or non-thinking models", () => {
    expect(thinkToSend(undefined, { on: true, level: null })).toBeNull();
    expect(thinkToSend(caps(false), { on: true, level: "high" })).toBeNull();
  });

  it("sends an explicit boolean to boolean models", () => {
    expect(thinkToSend(caps(true), THINK_OFF)).toBe(false);
    expect(thinkToSend(caps(true), { on: true, level: null })).toBe(true);
  });

  it("sends only advertised levels", () => {
    const always = caps(true, levels(false));
    expect(thinkToSend(always, THINK_OFF)).toBe("medium");
    expect(thinkToSend(always, { on: true, level: "high" })).toBe("high");
    const narrow = caps(true, {
      kind: "levels",
      levels: ["low"],
      default: "low",
      canDisable: false,
    });
    expect(thinkToSend(narrow, { on: true, level: "high" })).toBe("low");
  });

  it("turns off a levels model only when it allows it", () => {
    expect(thinkToSend(caps(true, levels(true)), THINK_OFF)).toBe(false);
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
    expect(thinkToSend(custom, { on: true, level: "xhigh" })).toBe("xhigh");
    expect(thinkToSend(custom, THINK_OFF)).toBe("max");
  });
});
