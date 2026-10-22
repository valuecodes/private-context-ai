import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { canCompare, ComparePicker, togglePick } from "./compare-picker";

const render = (current: string): string =>
  renderToString(
    <ComparePicker
      models={["qwen3.5:4b", "qwen3.5:9b", "llama3.2"]}
      current={current}
      onCompare={() => undefined}
      onCancel={() => undefined}
    />
  );

describe("ComparePicker", () => {
  it("preselects the current model and waits for a second one", () => {
    const html = render("qwen3.5:9b");
    expect(html.match(/type="checkbox"/g)).toHaveLength(3);
    expect(html.match(/checked=""/g)).toHaveLength(1);
    expect(html).toMatch(/checked=""[^>]*\/>qwen3\.5:9b/);
    expect(html).toMatch(/disabled=""[^>]*>Compare<\/button>/);
  });

  it("preselects nothing for a model that is not installed", () => {
    expect(render("gone")).not.toContain('checked=""');
  });

  it("allows two or three models, never four", () => {
    let picked = togglePick(new Set(), "a", true);
    expect(canCompare(picked)).toBe(false);
    picked = togglePick(picked, "b", true);
    expect(canCompare(picked)).toBe(true);
    picked = togglePick(picked, "c", true);
    expect(canCompare(picked)).toBe(true);
    expect(togglePick(picked, "d", true)).toBe(picked);
    picked = togglePick(picked, "a", false);
    expect([...picked]).toEqual(["b", "c"]);
  });
});
