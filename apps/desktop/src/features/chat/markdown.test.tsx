import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Markdown } from "./markdown";

const render = (text: string, highlight?: boolean): string =>
  renderToString(<Markdown text={text} highlight={highlight} />);

describe("Markdown", () => {
  it("drops raw HTML such as script tags and img onerror", () => {
    const html = render(
      'Hi <script>alert(1)</script>\n\n<img src="x" onerror="alert(1)">\n\n<b>bold</b>'
    );
    expect(html).not.toContain("<script");
    expect(html).not.toContain("<img");
    expect(html).not.toContain("onerror");
    expect(html).not.toContain("<b>");
    expect(html).toContain("Hi");
  });

  it("renders an https link as inert text with a copy-link button", () => {
    const html = render("[docs](https://example.com/a)");
    expect(html).not.toContain("href=");
    expect(html).not.toContain("<a");
    expect(html).toMatch(/title="https:\/\/example.com\/a"[^>]*>docs/);
    expect(html).toContain('aria-label="Copy link"');
  });

  it("renders a javascript: link without href or copy-link button", () => {
    const html = render("[click](javascript:alert(1))");
    expect(html).not.toContain("href=");
    expect(html).not.toContain("javascript:");
    expect(html).not.toContain("Copy link");
    expect(html).toContain("click");
  });

  it("renders footnote links without a copy-link button", () => {
    const html = render("Claim[^1]\n\n[^1]: Source");
    expect(html).not.toContain("href=");
    expect(html).not.toContain("Copy link");
    expect(html).toContain("Source");
  });

  it("renders an image as its alt text", () => {
    const html = render("![a cat](https://example.com/cat.png)");
    expect(html).not.toContain("<img");
    expect(html).not.toContain("cat.png");
    expect(html).toContain("a cat");
  });

  it("highlights a fenced ts block and adds a Copy button", () => {
    const html = render("```ts\nconst x: number = 1;\n```");
    expect(html).toContain('class="hljs-keyword"');
    expect(html).toContain('aria-label="Copy code"');
    expect(html).toContain(">ts<");
  });

  it("leaves code plain when highlighting is off", () => {
    const html = render("```ts\nconst x = 1;\n```", false);
    expect(html).not.toContain("hljs-");
    expect(html).toContain("const x = 1;");
  });

  it("renders a GFM table", () => {
    const html = render("| a | b |\n| - | - |\n| 1 | 2 |");
    expect(html).toContain("<table");
    expect(html).toMatch(/<td[^>]*>1<\/td>/);
  });

  it("keeps inline code inline", () => {
    const html = render("Run `pnpm test` now");
    expect(html).toContain("<code>pnpm test</code>");
    expect(html).not.toContain("Copy code");
  });
});
