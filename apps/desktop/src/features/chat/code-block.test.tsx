import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { CodeBlock, HIGHLIGHT_MAX_CHARS, renderHast } from "./code-block";

const render = (language: string | null, text: string, highlight = true) =>
  renderToString(
    <CodeBlock language={language} text={text} highlight={highlight} />
  );

describe("CodeBlock", () => {
  it("falls back to plain text for an unknown language", () => {
    const html = render("no-such-language", "let x = 1");
    expect(html).not.toContain("hljs-");
    expect(html).toContain("let x = 1");
    expect(html).toContain(">no-such-language<");
  });

  it("labels code without a language as text", () => {
    expect(render(null, "plain")).toMatch(/<span>text<\/span>/);
  });

  it("highlights a known language", () => {
    expect(render("ts", "const x = 1")).toContain('class="hljs-keyword"');
  });

  it("does not highlight when highlighting is off", () => {
    expect(render("ts", "const x = 1", false)).not.toContain("hljs-");
  });

  it("does not highlight code longer than the limit", () => {
    const long = "const x = 1;\n".repeat(HIGHLIGHT_MAX_CHARS / 10);
    expect(render("ts", long)).not.toContain("hljs-");
  });
});

describe("renderHast", () => {
  it("keeps only classes, whatever the tree holds", () => {
    const html = renderToString(
      <>
        {renderHast({
          type: "root",
          children: [
            {
              type: "element",
              properties: {
                className: ["hljs-string"],
                onclick: "alert(1)",
                style: "color:red",
                href: "https://example.com",
              },
              children: [{ type: "text", value: "<b>x</b>" }],
            },
            { type: "comment", value: "hidden" },
          ],
        })}
      </>
    );
    expect(html).toBe('<span class="hljs-string">&lt;b&gt;x&lt;/b&gt;</span>');
  });
});
