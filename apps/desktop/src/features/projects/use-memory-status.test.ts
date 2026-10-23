import { describe, expect, it, vi } from "vitest";

import type { MemoryStatus } from "~/lib/types";

import { applyStatus, clearSaved, withCounts } from "./use-memory-status";

vi.mock("~/lib/ipc");

const status = (
  state: MemoryStatus["state"],
  conversationId = "a",
  pending = 1
): MemoryStatus => ({ projectId: "p", conversationId, state, pending });

describe("applyStatus", () => {
  it("shows running, then saved, with the event's count", () => {
    const running = applyStatus({}, status("running"));
    expect(running.p).toEqual({
      pending: 1,
      conversationId: "a",
      state: "running",
    });
    const saved = applyStatus(running, status("saved", "a", 0));
    expect(saved.p).toEqual({
      pending: 0,
      conversationId: "a",
      state: "saved",
    });
  });

  it("clears a chat's indicator when its summary is cancelled", () => {
    const running = applyStatus({}, status("running"));
    expect(applyStatus(running, status("cancelled")).p?.state).toBeNull();
  });

  it("keeps a failure shown when another chat is queued", () => {
    const failed = applyStatus({}, status("failed"));
    const queued = applyStatus(failed, status("queued", "b", 2));
    expect(queued.p).toEqual({
      pending: 2,
      conversationId: "a",
      state: "failed",
    });
    expect(applyStatus(failed, status("queued")).p?.state).toBeNull();
  });
});

describe("clearSaved", () => {
  it("takes away only the save it was set for", () => {
    const saved = applyStatus({}, status("saved"));
    expect(clearSaved(saved, "p", "a").p?.state).toBeNull();
    expect(clearSaved(saved, "p", "b")).toBe(saved);
    const running = applyStatus(saved, status("running"));
    expect(clearSaved(running, "p", "a")).toBe(running);
  });
});

describe("withCounts", () => {
  it("replaces every count, zero for projects Rust left out", () => {
    const shown = applyStatus({}, status("failed", "a", 3));
    const next = withCounts(shown, [{ projectId: "q", pending: 2 }]);
    expect(next.p).toEqual({
      pending: 0,
      conversationId: "a",
      state: "failed",
    });
    expect(next.q).toEqual({ pending: 2, conversationId: null, state: null });
  });
});
