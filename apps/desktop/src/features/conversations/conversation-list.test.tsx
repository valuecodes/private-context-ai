import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { Conversation } from "~/lib/types";

import { ConversationList } from "./conversation-list";

const conversation = (id: string, updatedAt: string): Conversation => ({
  id,
  title: id,
  model: "m",
  createdAt: updatedAt,
  updatedAt,
  settings: {
    systemPrompt: "",
    options: {},
    think: { kind: "off" },
    charts: false,
  },
  projectId: null,
});

const now = new Date(2026, 9, 24, 10);
const shown = [
  conversation("Fresh", now.toISOString()),
  conversation("Ancient", "x"),
];

const render = (searching: boolean) =>
  renderToString(
    <ConversationList
      shown={shown}
      searching={searching}
      now={now}
      item={({ id }) => <li key={id}>{id}</li>}
    />
  );

describe("ConversationList", () => {
  it("labels each date group's list with its heading", () => {
    const html = render(false);
    const heading =
      /<h3 id="([^"]+)"[^>]*>Today<\/h3><ul aria-labelledby="([^"]+)"[^>]*><li>Fresh</.exec(
        html
      );
    expect(heading?.[1]).toBeDefined();
    expect(heading?.[1]).toBe(heading?.[2]);
    expect(html).toMatch(/>Older<\/h3><ul[^>]*><li>Ancient</);
  });

  it("lists search matches flat, without headings", () => {
    const html = render(true);
    expect(html).not.toContain("<h3");
    expect(html).toMatch(
      /<ul aria-label="Search results"[^>]*><li>Fresh<\/li><li>Ancient<\/li><\/ul>/
    );
  });
});
