import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { ConversationSettings } from "~/lib/types";

import { ChatSettings, ChatSettingsPanel } from "./chat-settings";
import { DEFAULT_SETTINGS } from "./default-settings";

vi.mock("~/lib/ipc");

const panel = (settings: ConversationSettings, disabled = false) =>
  renderToString(
    <ChatSettingsPanel
      settings={settings}
      onChange={vi.fn()}
      disabled={disabled}
    />
  );

/** The panel of a chat in project Thesis; saved when `onPreview` is set. */
const inProject = (onPreview?: () => void) =>
  renderToString(
    <ChatSettingsPanel
      settings={DEFAULT_SETTINGS}
      onChange={vi.fn()}
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

  it("leaves Think and Charts to the composer", () => {
    const html = panel({ ...DEFAULT_SETTINGS, charts: true });
    expect(html).not.toContain("Thinking");
    expect(html).not.toContain('id="charts"');
  });

  it("shows the saved values", () => {
    const html = panel({
      systemPrompt: "Answer only in Finnish",
      options: { temperature: 0.7, num_ctx: 16_384 },
      think: { kind: "off" },
      charts: false,
      files: true,
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

  it("disables every control while the chat loads", () => {
    expect(panel(DEFAULT_SETTINGS, true)).toMatch(/<fieldset[^>]* disabled=""/);
  });
});

describe("ChatSettings", () => {
  it("is a labelled button that starts closed", () => {
    const html = renderToString(
      <ChatSettings
        settings={DEFAULT_SETTINGS}
        onChange={vi.fn()}
        disabled={false}
      />
    );
    expect(html).toContain('aria-label="Chat settings"');
    expect(html).not.toMatch(/<details[^>]* open=""/);
  });
});
