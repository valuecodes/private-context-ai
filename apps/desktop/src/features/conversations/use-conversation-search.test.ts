import { describe, expect, it, vi } from "vitest";

import type { Conversation } from "~/lib/types";

import { filterConversations } from "./use-conversation-search";

vi.mock("~/lib/ipc");

const conversation = (id: string, title: string): Conversation => ({
  id,
  title,
  model: "m",
  createdAt: "x",
  updatedAt: "x",
  settings: {
    systemPrompt: "",
    options: {},
    think: { kind: "off" },
    charts: false,
    files: true,
  },
  projectId: null,
});

const list = [
  conversation("a", "Trip to Lapland"),
  conversation("b", "Rust lifetimes"),
  conversation("c", "Groceries"),
];

const ids = (conversations: Conversation[]): string[] =>
  conversations.map(({ id }) => id);

describe("filterConversations", () => {
  it("keeps everything for an empty query", () => {
    expect(filterConversations(list, "  ", null)).toBe(list);
  });

  it("matches titles at once, ignoring case", () => {
    expect(ids(filterConversations(list, "lapLAND", null))).toEqual(["a"]);
    expect(ids(filterConversations(list, "zzz", null))).toEqual([]);
  });

  it("adds content matches for the same query, in list order", () => {
    const matches = { query: "borrow", ids: new Set(["c", "b"]) };
    expect(ids(filterConversations(list, " borrow ", matches))).toEqual([
      "b",
      "c",
    ]);
  });

  it("ignores content matches of an earlier query", () => {
    const matches = { query: "bor", ids: new Set(["c"]) };
    expect(ids(filterConversations(list, "borrow", matches))).toEqual([]);
  });
});
