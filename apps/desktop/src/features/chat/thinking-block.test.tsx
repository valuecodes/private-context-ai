import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ThinkingBlock } from "./thinking-block";

describe("ThinkingBlock", () => {
  it("is open and says Thinking… while it streams", () => {
    const html = renderToString(
      <ThinkingBlock text="Let me see" ms={null} active />
    );
    expect(html).toContain("Thinking…");
    expect(html).toMatch(/<details[^>]* open=""/);
    expect(html).toContain("Let me see");
  });

  it("collapses to Thought for Ns once answered", () => {
    const html = renderToString(
      <ThinkingBlock text="Let me see" ms={3200} active={false} />
    );
    expect(html).toContain("Thought for 3s");
    expect(html).not.toMatch(/<details[^>]* open=""/);
  });

  it("never says 0s, and says Thoughts when the time is unknown", () => {
    expect(
      renderToString(<ThinkingBlock text="x" ms={120} active={false} />)
    ).toContain("Thought for 1s");
    expect(
      renderToString(<ThinkingBlock text="x" ms={null} active={false} />)
    ).toContain("Thoughts");
  });

  it("shows thinking as plain text, not Markdown", () => {
    const html = renderToString(
      <ThinkingBlock text="**not bold** <b>x</b>" ms={1000} active={false} />
    );
    expect(html).toContain("**not bold** &lt;b&gt;x&lt;/b&gt;");
    expect(html).not.toContain("<strong>");
  });
});
