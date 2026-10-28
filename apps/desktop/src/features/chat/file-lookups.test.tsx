import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { FileLookups } from "./file-lookups";

describe("FileLookups", () => {
  it("is collapsed and counts the lookups", () => {
    const html = renderToString(
      <FileLookups
        lookups={[
          { tool: "search", target: "tax", hits: 2 },
          { tool: "read", target: "a.txt", hits: 1 },
        ]}
      />
    );
    expect(html).toContain("Searched files (<!-- -->2<!-- -->)");
    expect(html).not.toMatch(/<details[^>]* open=""/);
  });

  it("describes each kind of lookup, and why one failed", () => {
    const html = renderToString(
      <FileLookups
        lookups={[
          { tool: "list", target: "page 1", hits: 12 },
          { tool: "search", target: "tax", hits: 1 },
          { tool: "read", target: "a.txt", hits: 40 },
          {
            tool: "read",
            target: "gone.txt",
            hits: 0,
            error: "no file has that name",
          },
          { tool: "unknown", target: "", hits: 0, error: "unknown tool" },
        ]}
      />
    );
    expect(html).toContain("Listed files page 1 — 12 names");
    expect(html).toContain("Searched “tax” — 1 passage");
    expect(html).toContain("Read a.txt — 40 lines");
    expect(html).toContain("Read gone.txt (failed: no file has that name)");
    expect(html).toContain("Called an unknown tool (failed: unknown tool)");
  });

  it("shows the model's text as plain text", () => {
    const html = renderToString(
      <FileLookups
        lookups={[{ tool: "search", target: "<b>x</b>", hits: 0 }]}
      />
    );
    expect(html).toContain("&lt;b&gt;x&lt;/b&gt;");
    expect(html).not.toContain("<b>");
  });
});
