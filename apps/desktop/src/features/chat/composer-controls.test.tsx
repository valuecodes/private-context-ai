import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type {
  ConversationSettings,
  ModelCapabilities,
  ThinkStyle,
} from "~/lib/types";

import { ComposerControls } from "./composer-controls";
import { DEFAULT_SETTINGS } from "./default-settings";
import { ThinkLevelMenu } from "./think-control";

vi.mock("~/lib/ipc");

const caps = (changes: Partial<ModelCapabilities>): ModelCapabilities => ({
  thinking: true,
  vision: false,
  tools: true,
  embeddingOnly: false,
  thinkStyle: { kind: "bool" },
  ...changes,
});

const levels = (canDisable: boolean): ThinkStyle => ({
  kind: "levels",
  levels: ["low", "medium", "high"],
  default: "medium",
  canDisable,
});

const controls = (
  modelCaps: ModelCapabilities | undefined,
  settings: ConversationSettings = DEFAULT_SETTINGS
) =>
  renderToString(
    <ComposerControls
      models={[{ name: "m", sizeBytes: null, parameterSize: null }]}
      capabilities={modelCaps === undefined ? {} : { m: modelCaps }}
      selected="m"
      onSelect={vi.fn()}
      onOpenPicker={vi.fn()}
      settings={settings}
      onSettings={vi.fn()}
      disabled={false}
    />
  );

describe("ComposerControls", () => {
  it("offers Instant and Thinking for an on/off model", () => {
    const off = controls(caps({}));
    expect(off).toMatch(/role="group" aria-label="Thinking"/);
    expect(off).toMatch(/aria-pressed="true"[^>]*>Instant</);
    expect(off).toMatch(/aria-pressed="false"[^>]*>Thinking</);
    const on = controls(caps({}), {
      ...DEFAULT_SETTINGS,
      think: { kind: "on" },
    });
    expect(on).toMatch(/aria-pressed="false"[^>]*>Instant</);
    expect(on).toMatch(/aria-pressed="true"[^>]*>Thinking</);
  });

  it("shows the chosen level, or Instant, on a level model's pill", () => {
    const high = controls(caps({ thinkStyle: levels(true) }), {
      ...DEFAULT_SETTINGS,
      think: { kind: "level", level: "high" },
    });
    expect(high).toMatch(
      /aria-label="Thinking: High"[^>]*aria-haspopup="menu"/
    );
    const instant = controls(caps({ thinkStyle: levels(true) }));
    expect(instant).toMatch(/aria-label="Instant"[^>]*aria-haspopup="menu"/);
  });

  it("says a model that always thinks has no Instant", () => {
    const html = controls(caps({ thinkStyle: levels(false) }));
    // Off falls back to the default level.
    expect(html).toMatch(
      /aria-label="Thinking: Medium" title="This model always thinks"/
    );
  });

  it("shows no think control for other models, or before caps are known", () => {
    expect(controls(caps({ thinking: false }))).not.toContain("Instant");
    expect(controls(undefined)).not.toContain("Instant");
    expect(controls(undefined)).not.toContain('aria-label="Thinking');
  });

  it("disables Charts without tools and shows it pressed when on", () => {
    expect(controls(caps({ tools: false }))).toMatch(
      /aria-label="Charts" aria-pressed="false" disabled=""/
    );
    expect(controls(undefined)).toMatch(/aria-label="Charts"[^>]*disabled=""/);
    const on = controls(caps({}), { ...DEFAULT_SETTINGS, charts: true });
    expect(on).toMatch(/aria-label="Charts" aria-pressed="true"/);
    expect(on).not.toMatch(/aria-label="Charts"[^>]*disabled=""/);
  });
});

describe("ThinkLevelMenu", () => {
  it("lists Instant only when given, and checks the selected item", () => {
    const html = renderToString(
      <ThinkLevelMenu
        options={[null, "low", "high"]}
        selected="high"
        onPick={vi.fn()}
      />
    );
    expect(html).toMatch(/role="menu" aria-label="Thinking level"/);
    expect(html).toMatch(
      /<button type="button" role="menuitemradio" aria-checked="false"[^>]*>.*Instant/
    );
    expect(html).toMatch(/aria-checked="true"[^>]*>.*High/);
    const always = renderToString(
      <ThinkLevelMenu
        options={["low", "high"]}
        selected="low"
        onPick={vi.fn()}
      />
    );
    expect(always).not.toContain("Instant");
  });
});
