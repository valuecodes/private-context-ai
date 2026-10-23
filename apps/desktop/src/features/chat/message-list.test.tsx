import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { UiMessage } from "./chat-reducer";
import { MessageList, STREAM_MARKDOWN_MAX_CHARS } from "./message-list";

const message = (
  role: UiMessage["role"],
  content: string,
  status: UiMessage["status"] = "complete"
): UiMessage => ({ id: `${role}-${content}`, role, content, status });

const render = (messages: UiMessage[], busy = false): string =>
  renderToString(
    <MessageList
      messages={messages}
      busy={busy}
      onRegenerate={() => undefined}
      onEdit={() => true}
    />
  );

const count = (html: string, text: string): number =>
  html.split(text).length - 1;

describe("MessageList", () => {
  it("renders nothing without messages", () => {
    expect(render([])).toBe("");
  });

  it("shows the typing indicator while waiting for the first token", () => {
    const html = render([message("assistant", "", "streaming")]);
    expect(html).toContain("Assistant is typing");
    expect(html).not.toContain("Copy message");
  });

  it("shows thinking above the reply, without the typing indicator", () => {
    const thinking: UiMessage = {
      ...message("assistant", "", "streaming"),
      thinking: "Pondering",
    };
    const html = render([thinking]);
    expect(html).toContain("Thinking…");
    expect(html).toContain("Pondering");
    expect(html).not.toContain("Assistant is typing");

    const answered = render([
      {
        ...message("assistant", "Done"),
        thinking: "Pondering",
        thinkingMs: 2000,
      },
    ]);
    expect(answered).toContain("Thought for 2s");
    expect(answered.indexOf("Pondering")).toBeLessThan(
      answered.indexOf("Done")
    );
  });

  it("renders a user message as plain text", () => {
    const html = render([message("user", "**not bold** <b>x</b>")]);
    expect(html).toContain("**not bold** &lt;b&gt;x&lt;/b&gt;");
    expect(html).not.toContain("<strong>");
    expect(html).toContain('aria-label="Copy message"');
  });

  it("renders an assistant reply as Markdown", () => {
    const html = render([message("assistant", "**bold** and `code`")]);
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<code>code</code>");
  });

  it("shows the stats of a finished reply on hover", () => {
    const stats = { completionTokens: 100, durationMs: 2500, evalMs: 2000 };
    const html = render([{ ...message("assistant", "Done"), stats }]);
    expect(html).toContain('title="100 tokens · 50 tok/s · 2.5s"');
    expect(html).toContain('aria-label="100 tokens · 50 tok/s · 2.5s"');

    for (const status of ["streaming", "cancelled"] as const) {
      const unfinished = render([
        { ...message("assistant", "Part", status), stats },
      ]);
      expect(unfinished).not.toContain("tok/s");
    }
    expect(render([message("assistant", "Old")])).not.toContain("tok/s");
  });

  it("keeps the Stopped and Failed notes", () => {
    const html = render([
      message("assistant", "partial", "cancelled"),
      message("user", "prompt", "error"),
    ]);
    expect(html).toContain(">Stopped<");
    expect(html).toContain(">Failed<");
  });

  it("shows a long streaming reply as plain text until it completes", () => {
    const long = `**bold** ${"x".repeat(STREAM_MARKDOWN_MAX_CHARS)}`;
    const streaming = render([message("assistant", long, "streaming")]);
    expect(streaming).toContain("**bold**");
    expect(streaming).not.toContain("<strong>");
    expect(render([message("assistant", long)])).toContain(
      "<strong>bold</strong>"
    );
  });

  it("offers Regenerate on the last reply only", () => {
    const html = render([
      message("user", "One"),
      message("assistant", "First"),
      message("user", "Two"),
      message("assistant", "Second"),
    ]);
    expect(count(html, 'aria-label="Regenerate reply"')).toBe(1);
    expect(html.indexOf("Regenerate reply")).toBeGreaterThan(
      html.indexOf("Second")
    );
    expect(
      render([message("assistant", "Hi"), message("user", "Q", "error")])
    ).not.toContain("Regenerate reply");
  });

  it("offers Edit on saved prompts, not on failed ones", () => {
    const html = render([
      message("user", "Saved"),
      message("assistant", "Reply"),
      message("user", "Unsaved", "error"),
    ]);
    expect(count(html, 'aria-label="Edit message"')).toBe(1);
    // Sent here, but Rust's id has not arrived yet.
    const local: UiMessage = { ...message("user", "New"), local: true };
    expect(render([local])).not.toContain("Edit message");
  });

  it("disables Regenerate and Edit while busy", () => {
    const html = render(
      [message("user", "One"), message("assistant", "Reply")],
      true
    );
    expect(html).toMatch(/disabled=""[^>]*aria-label="Edit message"/);
    expect(html).toMatch(/disabled=""[^>]*aria-label="Regenerate reply"/);
    expect(
      render([message("user", "One"), message("assistant", "Reply")])
    ).not.toContain('disabled=""');
  });

  it("does not highlight code in a streaming reply", () => {
    const fence = "```ts\nconst x = 1;\n```";
    expect(render([message("assistant", fence, "streaming")])).not.toContain(
      "hljs-"
    );
    expect(render([message("assistant", fence)])).toContain("hljs-");
  });

  it("shows the text before a chart, the chart, then the rest", () => {
    const charted: UiMessage = {
      ...message("assistant", "Before text.After text."),
      charts: [
        {
          offset: 12,
          spec: { title: "Pop", type: "line", labels: ["a"], values: [1] },
        },
        { offset: 12, spec: null, error: "a chart has 1 to 100 values" },
      ],
    };
    const html = render([charted]);
    const before = html.indexOf("Before text.");
    const chart = html.indexOf("<svg");
    const failed = html.indexOf("Chart could not be shown");
    const after = html.indexOf("After text.");
    expect(before).toBeGreaterThan(-1);
    expect(chart).toBeGreaterThan(before);
    expect(failed).toBeGreaterThan(chart);
    expect(after).toBeGreaterThan(failed);
    expect(html).not.toContain("Before text.After");
  });

  it("waits for the commentary under a chart while it streams", () => {
    const html = render([
      {
        ...message("assistant", "", "streaming"),
        charts: [
          {
            offset: 0,
            spec: { title: "Pop", type: "bar", labels: ["a"], values: [1] },
          },
        ],
      },
    ]);
    expect(html.indexOf("<svg")).toBeLessThan(
      html.indexOf("Assistant is typing")
    );
  });

  it("labels a reply with its model, and older replies with none", () => {
    const labelled: UiMessage = {
      ...message("assistant", "Reply"),
      model: "qwen3.5:9b",
    };
    expect(render([labelled])).toContain(">qwen3.5:9b</span>");
    const older = render([message("assistant", "Old")]);
    expect(older).not.toContain("qwen");
    expect(older).not.toContain("mb-1 block text-xs");
  });
});
