import { describe, expect, it } from "vitest";

import type { Conversation } from "~/lib/types";

import { groupByDate } from "./date-groups";

const at = (id: string, updatedAt: string): Conversation => ({
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

// Local times, so the tests hold in any time zone.
const local = (day: number, ms = 0): string =>
  new Date(new Date(2026, 9, day).getTime() + ms).toISOString();

const NOW = new Date(2026, 9, 24, 10);

const groups = (conversations: Conversation[]) =>
  groupByDate(conversations, NOW).map(({ label, conversations: list }) => [
    label,
    list.map(({ id }) => id),
  ]);

describe("groupByDate", () => {
  it("splits today from yesterday at local midnight", () => {
    expect(
      groups([at("today", local(24)), at("yesterday", local(24, -1))])
    ).toEqual([
      ["Today", ["today"]],
      ["Yesterday", ["yesterday"]],
    ]);
  });

  it("puts the start of 7 and 30 days ago in the nearer group", () => {
    expect(
      groups([
        at("7", local(17)),
        at("8", local(17, -1)),
        at("30", local(24 - 30)),
        at("31", local(24 - 30, -1)),
      ])
    ).toEqual([
      ["Previous 7 days", ["7"]],
      ["Previous 30 days", ["8", "30"]],
      ["Older", ["31"]],
    ]);
  });

  it("counts a future time as today and an unreadable one as older", () => {
    expect(groups([at("bad", "x"), at("future", local(30))])).toEqual([
      ["Today", ["future"]],
      ["Older", ["bad"]],
    ]);
  });

  it("keeps the order within a group and drops empty groups", () => {
    expect(
      groups([
        at("a", local(24, 5)),
        at("old", local(24 - 40)),
        at("b", local(24, 9)),
      ])
    ).toEqual([
      ["Today", ["a", "b"]],
      ["Older", ["old"]],
    ]);
    expect(groupByDate([], NOW)).toEqual([]);
  });
});
