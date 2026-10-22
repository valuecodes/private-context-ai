import type { ModelCapabilities, Think, ThinkLevel } from "~/lib/types";

/**
 * What the user picked: thinking on or off, and a level for models that take
 * one. Off by default.
 */
type ThinkChoice = { on: boolean; level: ThinkLevel | null };

const THINK_OFF: ThinkChoice = { on: false, level: null };

/**
 * The `think` value `chat_send` gets, or `null` when the model cannot think
 * or its capabilities are not known yet. Rust checks it again against the
 * model and always sends an explicit value to a thinking model.
 */
const thinkToSend = (
  caps: ModelCapabilities | undefined,
  choice: ThinkChoice
): Think | null => {
  if (caps === undefined || !caps.thinking) {
    return null;
  }
  const style = caps.thinkStyle;
  if (style.kind === "bool") {
    return choice.on;
  }
  if (!choice.on && style.canDisable) {
    return false;
  }
  return choice.level !== null && style.levels.includes(choice.level)
    ? choice.level
    : style.default;
};

/** The level the control shows as selected for a levels model; null is Off. */
const selectedLevel = (
  caps: ModelCapabilities,
  choice: ThinkChoice
): ThinkLevel | null => {
  const think = thinkToSend(caps, choice);
  return typeof think === "string" ? think : null;
};

export type { ThinkChoice };
export { selectedLevel, THINK_OFF, thinkToSend };
