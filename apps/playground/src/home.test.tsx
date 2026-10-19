import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { commands, stack } from "~/content";

import { Home } from "./home";

describe("Home", () => {
  const html = renderToString(<Home />);

  it("renders the heading", () => {
    expect(html).toMatch(/<h1[^>]*>Agentic Monorepo Starter<\/h1>/);
  });

  it("renders every stack item", () => {
    for (const item of stack) {
      expect(html).toContain(item.name);
    }
  });

  it("renders a copy button per quick-start command", () => {
    expect(html.match(/<button/g)).toHaveLength(commands.length);
  });

  it("gives every copy button a status region for its result", () => {
    expect(html.match(/role="status"/g)).toHaveLength(commands.length);
  });
});
