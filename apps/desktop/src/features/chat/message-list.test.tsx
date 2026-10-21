import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { MessageList, STREAM_MARKDOWN_MAX_CHARS } from "./message-list";
import type { UiMessage } from "./use-chat-stream";

const message = (
  role: UiMessage["role"],
  content: string,
  status: UiMessage["status"] = "complete"
): UiMessage => ({ id: `${role}-${content}`, role, content, status });

const render = (messages: UiMessage[]): string =>
  renderToString(<MessageList messages={messages} />);

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

  it("does not highlight code in a streaming reply", () => {
    const fence = "```ts\nconst x = 1;\n```";
    expect(render([message("assistant", fence, "streaming")])).not.toContain(
      "hljs-"
    );
    expect(render([message("assistant", fence)])).toContain("hljs-");
  });
});
