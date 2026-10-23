import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { ModelCapabilities, ModelInfo } from "~/lib/types";

import {
  activeName,
  ModelOptions,
  ModelPicker,
  nextIndex,
} from "./model-picker";

vi.mock("~/lib/ipc");

const model = (name: string, parameterSize: string | null = null) => ({
  name,
  sizeBytes: null,
  parameterSize,
});

const qwen3 = model("qwen3:8b", "8.2B");
const gemma = model("gemma3:4b", "4.3B");

const caps = (changes: Partial<ModelCapabilities>): ModelCapabilities => ({
  thinking: false,
  vision: false,
  tools: false,
  embeddingOnly: false,
  thinkStyle: { kind: "bool" },
  ...changes,
});

const picker = (models: ModelInfo[], selected: string) =>
  renderToString(
    <ModelPicker
      models={models}
      capabilities={{}}
      selected={selected}
      onSelect={vi.fn()}
      onOpen={vi.fn()}
      disabled={false}
    />
  );

const options = (
  visible: ModelInfo[],
  capabilities: Record<string, ModelCapabilities> = {},
  installed = true
) =>
  renderToString(
    <ModelOptions
      id="models"
      visible={visible}
      installed={installed}
      selected="qwen3:8b"
      active="gemma3:4b"
      capabilities={capabilities}
      onPick={vi.fn()}
    />
  );

describe("ModelPicker", () => {
  it("shows the selected model and its size on a closed pill", () => {
    const html = picker([qwen3, gemma], "qwen3:8b");
    expect(html).toMatch(
      /<button type="button" aria-label="Model: qwen3:8b"[^>]*aria-haspopup="listbox" aria-expanded="false"/
    );
    expect(html).toContain("8.2B");
    expect(html).not.toContain('role="listbox"');
  });

  it("says No models when nothing is installed", () => {
    expect(picker([], "")).toMatch(/aria-label="No models"/);
  });
});

describe("ModelOptions", () => {
  it("lists the models as options, marking the selected one", () => {
    const html = options([qwen3, gemma]);
    expect(html).toMatch(/<ul id="models" role="listbox" aria-label="Models"/);
    expect(html).toMatch(
      /<li id="models-0" role="option" aria-selected="true"[^>]*>.*qwen3:8b/
    );
    expect(html).toMatch(
      /<li id="models-1" role="option" aria-selected="false"/
    );
  });

  it("shows a badge per known capability", () => {
    const html = options([qwen3, gemma], {
      "qwen3:8b": caps({ thinking: true, tools: true }),
      "gemma3:4b": caps({ vision: true }),
    });
    const [first, second] = html.split('id="models-1"');
    expect(first).toContain('title="Thinking"');
    expect(first).toContain('title="Tools"');
    expect(first).not.toContain('title="Vision"');
    expect(second).toContain('title="Vision"');
    expect(second).not.toContain('title="Thinking"');
  });

  it("says when nothing matches, or nothing is installed", () => {
    expect(options([])).toContain("No matching models");
    expect(options([], {}, false)).toContain("No chat models installed");
  });
});

describe("nextIndex", () => {
  it("wraps in both directions", () => {
    expect(nextIndex(2, 1, 3)).toBe(0);
    expect(nextIndex(0, -1, 3)).toBe(2);
    expect(nextIndex(1, 1, 3)).toBe(2);
  });

  it("starts from the ends when nothing is active", () => {
    expect(nextIndex(-1, 1, 3)).toBe(0);
    expect(nextIndex(-1, -1, 3)).toBe(2);
  });

  it("returns -1 for an empty list", () => {
    expect(nextIndex(0, 1, 0)).toBe(-1);
  });
});

describe("activeName", () => {
  it("keeps the active model while it is shown", () => {
    expect(activeName([qwen3, gemma], "gemma3:4b")).toBe("gemma3:4b");
  });

  it("falls back to the first shown model when it is filtered out", () => {
    expect(activeName([gemma], "qwen3:8b")).toBe("gemma3:4b");
    expect(activeName([gemma], null)).toBe("gemma3:4b");
  });

  it("is null when nothing is shown", () => {
    expect(activeName([], "qwen3:8b")).toBeNull();
  });
});
