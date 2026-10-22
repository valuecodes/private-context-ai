import type { ModelCapabilities, ThinkChoice, ThinkLevel } from "~/lib/types";

const THINK_OFF: ThinkChoice = { kind: "off" };

/**
 * The level the control shows as selected for a levels model; null is Off.
 * Mirrors Rust's `resolve_think`: an unknown level or plain "on" is the
 * default level, and Off is the default level for a model that always
 * thinks.
 */
const selectedLevel = (
  caps: ModelCapabilities,
  choice: ThinkChoice
): ThinkLevel | null => {
  const style = caps.thinkStyle;
  if (style.kind === "bool") {
    return null;
  }
  if (choice.kind === "off") {
    return style.canDisable ? null : style.default;
  }
  return choice.kind === "level" && style.levels.includes(choice.level)
    ? choice.level
    : style.default;
};

export { selectedLevel, THINK_OFF };
