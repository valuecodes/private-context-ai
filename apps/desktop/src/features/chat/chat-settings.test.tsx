import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { ConversationSettings, ModelCapabilities } from "~/lib/types";

import { ChatSettings, ChatSettingsPanel } from "./chat-settings";
import { DEFAULT_SETTINGS } from "./default-settings";

vi.mock("~/lib/ipc");

const qwen3: ModelCapabilities = {
  thinking: true,
  vision: false,
  tools: true,
  embeddingOnly: false,
  thinkStyle: { kind: "bool" },
};

const panel = (
  settings: ConversationSettings,
  caps: ModelCapabilities | undefined = undefined,
  disabled = false
) =>
  renderToString(
    <ChatSettingsPanel
      settings={settings}
      onChange={vi.fn()}
      caps={caps}
      disabled={disabled}
    />
  );

/** The panel of a chat in project Thesis; saved when `onPreview` is set. */
const inProject = (onPreview?: () => void) =>
  renderToString(
    <ChatSettingsPanel
      settings={DEFAULT_SETTINGS}
      onChange={vi.fn()}
      caps={undefined}
      disabled={false}
      project={{ name: "Thesis", onOpen: vi.fn(), onPreview }}
    />
  );

describe("ChatSettingsPanel", () => {
  it("says project instructions apply only in a project chat", () => {
    expect(panel(DEFAULT_SETTINGS)).not.toContain("Project instructions");
    const html = inProject();
    expect(html).toContain("Project instructions and memory apply");
    expect(html).toMatch(/<button type="button"[^>]*>Open Thesis</);
    // Only a saved chat offers its preview.
    expect(html).not.toContain("What this chat sees");
    expect(inProject(vi.fn())).toMatch(
      /<button type="button"[^>]*>What this chat sees</
    );
  });

  it("shows the defaults when nothing is set", () => {
    const html = panel(DEFAULT_SETTINGS);
    expect(html).toContain("System prompt");
    expect(html).toMatch(/<textarea[^>]*id="system-prompt"[^>]*><\/textarea>/);
    expect(html).toContain("Default");
    expect(html).not.toContain('aria-label="Reset temperature"');
    expect(html).toMatch(/<option value="" selected="">Model default/);
    expect(html).toContain("8k tokens");
    expect(html).toContain(
      "Larger contexts use more memory and make the model slower to load."
    );
  });

  it("disables the Charts switch for a model without tools", () => {
    const off = panel(DEFAULT_SETTINGS, { ...qwen3, tools: false });
    expect(off).toMatch(/<input[^>]*id="charts"[^>]*disabled=""/);
    expect(off).toContain("doesn&#x27;t support tools");
    const unknown = panel(DEFAULT_SETTINGS);
    expect(unknown).toMatch(/<input[^>]*id="charts"[^>]*disabled=""/);

    const on = panel({ ...DEFAULT_SETTINGS, charts: true }, qwen3);
    expect(on).toMatch(/<input[^>]*id="charts"[^>]*checked=""/);
    expect(on).not.toMatch(/<input[^>]*id="charts"[^>]*disabled=""/);
  });

  it("shows the saved values", () => {
    const html = panel({
      systemPrompt: "Answer only in Finnish",
      options: { temperature: 0.7, num_ctx: 16_384 },
      think: { kind: "off" },
      charts: false,
    });
    expect(html).toContain("Answer only in Finnish</textarea>");
    expect(html).toContain("0.7");
    expect(html).toContain('aria-label="Reset temperature"');
    expect(html).toMatch(/<option value="16384" selected="">16k tokens/);
  });

  it("keeps a context size that is not in the list", () => {
    const html = panel({
      ...DEFAULT_SETTINGS,
      options: { num_ctx: 6000 },
    });
    expect(html).toMatch(/<option value="6000" selected="">6000 tokens/);
  });

  it("shows the Think control only for thinking models", () => {
    expect(panel(DEFAULT_SETTINGS)).not.toContain("Thinking");
    expect(
      panel(DEFAULT_SETTINGS, { ...qwen3, thinking: false })
    ).not.toContain("Thinking");
    const html = panel({ ...DEFAULT_SETTINGS, think: { kind: "on" } }, qwen3);
    expect(html).toContain("Thinking");
    expect(html).toContain('aria-pressed="true"');
  });

  it("disables every control while the chat loads", () => {
    expect(panel(DEFAULT_SETTINGS, qwen3, true)).toMatch(
      /<fieldset[^>]* disabled=""/
    );
  });
});

describe("ChatSettings", () => {
  it("is a labelled button that starts closed", () => {
    const html = renderToString(
      <ChatSettings
        settings={DEFAULT_SETTINGS}
        onChange={vi.fn()}
        caps={undefined}
        disabled={false}
      />
    );
    expect(html).toContain('aria-label="Chat settings"');
    expect(html).not.toMatch(/<details[^>]* open=""/);
  });
});
