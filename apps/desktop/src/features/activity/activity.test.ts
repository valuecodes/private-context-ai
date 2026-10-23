import { describe, expect, it } from "vitest";

import type { AuditEvent } from "~/lib/types";

import {
  formatDuration,
  formatTokens,
  hostsSummary,
  kindLabel,
  statusLabel,
} from "./activity";

describe("kindLabel", () => {
  it("names memory summaries and leaves other kinds as they are", () => {
    expect(kindLabel("ollama.memory")).toBe("Memory summary");
    expect(kindLabel("ollama.chat")).toBe("ollama.chat");
  });
});

const event = (overrides: Partial<AuditEvent>): AuditEvent => ({
  id: 1,
  at: "2026-10-21T08:00:00.000Z",
  kind: "ollama.chat",
  host: "127.0.0.1:11434",
  endpoint: "/api/chat",
  model: "llama3.2",
  status: "ok",
  promptTokens: null,
  completionTokens: null,
  durationMs: null,
  detail: null,
  ...overrides,
});

describe("hostsSummary", () => {
  it("lists the hosts it is given", () => {
    expect(hostsSummary(["127.0.0.1:11434", "[::1]:11434"])).toBe(
      "Hosts contacted: 127.0.0.1:11434, [::1]:11434"
    );
  });

  it("says none when nothing was contacted", () => {
    expect(hostsSummary([])).toBe("Hosts contacted: none");
  });
});

describe("formatTokens", () => {
  it("shows prompt and completion counts", () => {
    expect(formatTokens(event({ promptTokens: 3, completionTokens: 2 }))).toBe(
      "3 / 2"
    );
    expect(formatTokens(event({ promptTokens: 3 }))).toBe("3 / ?");
  });

  it("shows a dash when no counts are known", () => {
    expect(formatTokens(event({}))).toBe("—");
  });
});

describe("formatDuration", () => {
  it("uses milliseconds below a second and seconds above", () => {
    expect(formatDuration(null)).toBe("—");
    expect(formatDuration(42)).toBe("42 ms");
    expect(formatDuration(1500)).toBe("1.5 s");
  });
});

describe("statusLabel", () => {
  it("names an attempt without an outcome", () => {
    expect(statusLabel(event({ status: "attempt" }))).toBe("no outcome");
  });

  it("adds the detail of a failure", () => {
    expect(
      statusLabel(event({ status: "error", detail: "ollama, HTTP 404" }))
    ).toBe("error (ollama, HTTP 404)");
  });
});
