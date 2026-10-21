import type { ModelInfo } from "~/lib/types";

/**
 * The model to chat with: `current` (the opened conversation's or the user's
 * pick) if installed, else `fallback` (the default from Settings) if
 * installed, else the first listed one. Derived at render, so it does not
 * matter whether the settings or the model list loaded first.
 */
const pickModel = (
  models: ModelInfo[],
  current: string,
  fallback: string
): string => {
  const installed = (name: string) =>
    name !== "" && models.some((model) => model.name === name);
  if (installed(current)) {
    return current;
  }
  if (installed(fallback)) {
    return fallback;
  }
  return models[0]?.name ?? "";
};

export { pickModel };
