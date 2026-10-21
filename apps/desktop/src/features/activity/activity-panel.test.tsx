import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { AuditEvent } from "~/lib/types";

import { ActivityTable } from "./activity-panel";

const event = (id: number, overrides: Partial<AuditEvent>): AuditEvent => ({
  id,
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

describe("ActivityTable", () => {
  it("renders rows in the order given, newest first", () => {
    const html = renderToString(
      <ActivityTable
        events={[
          event(3, {
            kind: "ollama.chat",
            promptTokens: 3,
            completionTokens: 2,
          }),
          event(2, { kind: "ollama.tags", model: null }),
          event(1, { kind: "app.start", host: null, model: null }),
        ]}
        hosts={["127.0.0.1:11434"]}
      />
    );
    const chat = html.indexOf("ollama.chat");
    const tags = html.indexOf("ollama.tags");
    const start = html.indexOf("app.start");
    expect(chat).toBeGreaterThan(-1);
    expect(chat).toBeLessThan(tags);
    expect(tags).toBeLessThan(start);
    expect(html).toContain("3 / 2");
  });

  it("takes the summary from the hosts it is given, not the rows", () => {
    const html = renderToString(
      <ActivityTable
        events={[event(1, {})]}
        hosts={["127.0.0.1:11434", "[::1]:11434"]}
      />
    );
    expect(html).toContain("Hosts contacted: 127.0.0.1:11434, [::1]:11434");
  });

  it("shows an empty state", () => {
    const html = renderToString(<ActivityTable events={[]} hosts={[]} />);
    expect(html).toContain("Hosts contacted: none");
    expect(html).toContain("No requests recorded yet.");
  });

  it("marks an attempt without an outcome", () => {
    const html = renderToString(
      <ActivityTable
        events={[event(1, { status: "attempt" })]}
        hosts={["127.0.0.1:11434"]}
      />
    );
    expect(html).toContain("no outcome");
  });
});
