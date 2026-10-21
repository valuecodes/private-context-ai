import { describe, expect, it } from "vitest";

import type { ModelInfo } from "~/lib/types";

import { pickModel } from "./pick-model";

const model = (name: string): ModelInfo => ({
  name,
  sizeBytes: null,
  parameterSize: null,
});

const installed = [model("a"), model("b"), model("c")];

describe("pickModel", () => {
  it("keeps the current model when it is installed", () => {
    expect(pickModel(installed, "c", "b")).toBe("c");
  });

  it("falls back to the default model", () => {
    expect(pickModel(installed, "", "b")).toBe("b");
    expect(pickModel(installed, "gone", "b")).toBe("b");
  });

  it("falls back to the first model when the default is not installed", () => {
    expect(pickModel(installed, "", "gone")).toBe("a");
    expect(pickModel(installed, "", "")).toBe("a");
  });

  it("is empty without models", () => {
    expect(pickModel([], "a", "b")).toBe("");
  });

  it("gives the same answer whichever of settings and models came first", () => {
    // Models first: the default is still "" until the settings arrive.
    const beforeSettings = pickModel(installed, "", "");
    const afterSettings = pickModel(installed, "", "b");
    // Settings first: the list is still empty until the models arrive.
    const beforeModels = pickModel([], "", "b");
    const afterModels = pickModel(installed, "", "b");

    expect(beforeSettings).toBe("a");
    expect(beforeModels).toBe("");
    expect(afterSettings).toBe("b");
    expect(afterModels).toBe("b");
  });
});
