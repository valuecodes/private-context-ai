import { describe, expect, it, vi } from "vitest";

import { ollamaStatus } from "~/lib/ipc";

import { describeStatus, isReachable, loadOllamaStatus } from "./status-state";

vi.mock("~/lib/ipc");

const running = {
  reachable: true,
  version: "0.12.3",
  baseUrl: "http://127.0.0.1:11434",
};

describe("loadOllamaStatus", () => {
  it("returns the status from the ollama_status command", async () => {
    vi.mocked(ollamaStatus).mockResolvedValueOnce(running);
    await expect(loadOllamaStatus()).resolves.toEqual({
      phase: "ready",
      status: running,
    });
  });

  it("returns the error when the command fails", async () => {
    vi.mocked(ollamaStatus).mockRejectedValueOnce({
      kind: "invalidBaseUrl",
      message: "host must be a loopback address",
    });
    await expect(loadOllamaStatus()).resolves.toEqual({
      phase: "failed",
      error: {
        kind: "invalidBaseUrl",
        message: "host must be a loopback address",
      },
    });
  });
});

describe("describeStatus", () => {
  it("says checking before the first answer", () => {
    expect(describeStatus({ phase: "checking" })).toEqual({
      text: "Ollama: checking…",
      canRetry: false,
    });
  });

  it("shows the version and address when running", () => {
    const state = { phase: "ready", status: running } as const;
    expect(describeStatus(state).text).toBe(
      "Ollama: running v0.12.3 at http://127.0.0.1:11434"
    );
    expect(isReachable(state)).toBe(true);
  });

  it("hints to start Ollama when it is not running", () => {
    const state = {
      phase: "ready",
      status: { ...running, reachable: false, version: null },
    } as const;
    expect(describeStatus(state)).toEqual({
      text: "Ollama: not running at http://127.0.0.1:11434. Start Ollama, then retry.",
      canRetry: true,
    });
    expect(isReachable(state)).toBe(false);
  });

  it("shows a rejected URL without offering a retry", () => {
    const state = {
      phase: "failed",
      error: { kind: "invalidBaseUrl", message: "scheme must be http" },
    } as const;
    expect(describeStatus(state)).toEqual({
      text: "Ollama URL rejected: scheme must be http",
      canRetry: false,
    });
    expect(isReachable(state)).toBe(false);
  });

  it("offers a retry for other failures", () => {
    expect(
      describeStatus({
        phase: "failed",
        error: { kind: "ollama", message: "HTTP 500" },
      })
    ).toEqual({ text: "Ollama: HTTP 500", canRetry: true });
  });
});
