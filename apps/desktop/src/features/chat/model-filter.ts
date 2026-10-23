import type { ModelInfo } from "~/lib/types";

/** The models whose name contains `query`, ignoring case; all when blank. */
const filterModels = (models: ModelInfo[], query: string): ModelInfo[] => {
  const needle = query.trim().toLowerCase();
  return needle === ""
    ? models
    : models.filter(({ name }) => name.toLowerCase().includes(needle));
};

export { filterModels };
