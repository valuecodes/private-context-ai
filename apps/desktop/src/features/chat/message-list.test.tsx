import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { MessageList } from "./message-list";
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

  it("does not highlight code in a streaming reply", () => {
    const fence = "```ts\nconst x = 1;\n```";
    expect(render([message("assistant", fence, "streaming")])).not.toContain(
      "hljs-"
    );
    expect(render([message("assistant", fence)])).toContain("hljs-");
  });
});
