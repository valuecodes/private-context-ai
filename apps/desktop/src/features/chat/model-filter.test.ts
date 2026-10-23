import { describe, expect, it } from "vitest";

import type { ModelInfo } from "~/lib/types";

import { filterModels } from "./model-filter";

const model = (name: string): ModelInfo => ({
  name,
  sizeBytes: null,
  parameterSize: null,
});

const models = [model("qwen3:8b"), model("Llama3.2:3b"), model("gemma3")];

describe("filterModels", () => {
  it("matches a substring of the name, ignoring case", () => {
    expect(filterModels(models, "LLAMA").map(({ name }) => name)).toEqual([
      "Llama3.2:3b",
    ]);
    expect(filterModels(models, "3:").map(({ name }) => name)).toEqual([
      "qwen3:8b",
    ]);
  });

  it("returns every model for a blank query", () => {
    expect(filterModels(models, "")).toEqual(models);
    expect(filterModels(models, "  ")).toEqual(models);
  });

  it("returns nothing when no name matches", () => {
    expect(filterModels(models, "mistral")).toEqual([]);
  });
});
