import { describe, expect, it, vi } from "vitest";

import type { ModelCapabilities } from "~/lib/types";

import type { CapabilitiesState } from "./use-model-capabilities";
import {
  capabilitiesReducer,
  INITIAL_CAPABILITIES,
  namesToStart,
} from "./use-model-capabilities";

vi.mock("~/lib/ipc");

const qwen3: ModelCapabilities = {
  thinking: true,
  vision: false,
  tools: true,
  embeddingOnly: false,
  thinkStyle: { kind: "bool" },
};

const started = (
  state: CapabilitiesState,
  names: string[]
): CapabilitiesState =>
  capabilitiesReducer(state, {
    type: "started",
    generation: state.generation,
    names,
  });

describe("capabilitiesReducer", () => {
  it("stores a result and clears its pending entry", () => {
    const state = capabilitiesReducer(started(INITIAL_CAPABILITIES, ["a"]), {
      type: "loaded",
      generation: 0,
      name: "a",
      caps: qwen3,
    });
    expect(state.known).toEqual({ a: qwen3 });
    expect(state.pending).toEqual([]);
  });

  it("drops a result from before a reset", () => {
    const before = started(INITIAL_CAPABILITIES, ["a"]);
    const reset = capabilitiesReducer(before, { type: "reset" });
    expect(reset.generation).toBe(1);
    expect(reset.pending).toEqual([]);
    const after = capabilitiesReducer(reset, {
      type: "loaded",
      generation: 0,
      name: "a",
      caps: qwen3,
    });
    expect(after).toBe(reset);
  });

  it("does not start a pending name twice", () => {
    const once = started(INITIAL_CAPABILITIES, ["a"]);
    expect(started(once, ["a"])).toBe(once);
    expect(started(once, ["a", "b"]).pending).toEqual(["a", "b"]);
  });

  it("clears the pending entry of a failed lookup and remembers it", () => {
    const state = capabilitiesReducer(started(INITIAL_CAPABILITIES, ["a"]), {
      type: "failed",
      generation: 0,
      name: "a",
    });
    expect(state.pending).toEqual([]);
    expect(state.failed).toEqual(["a"]);
    expect(state.known).toEqual({});
  });

  it("forgets what it knew and what failed on a reset", () => {
    const state: CapabilitiesState = {
      generation: 2,
      known: { a: qwen3 },
      pending: [],
      failed: ["b"],
    };
    expect(capabilitiesReducer(state, { type: "reset" })).toEqual({
      generation: 3,
      known: {},
      pending: [],
      failed: [],
    });
  });
});

describe("namesToStart", () => {
  const state: CapabilitiesState = {
    generation: 0,
    known: { known: qwen3 },
    pending: ["running"],
    failed: ["broken"],
  };

  it("skips known and running names, and duplicates", () => {
    expect(
      namesToStart(state, ["known", "running", "new", "new"], {
        retryFailed: false,
      })
    ).toEqual(["new"]);
  });

  it("retries a failed name only when asked", () => {
    expect(namesToStart(state, ["broken"], { retryFailed: false })).toEqual([]);
    expect(namesToStart(state, ["broken"], { retryFailed: true })).toEqual([
      "broken",
    ]);
  });
});
