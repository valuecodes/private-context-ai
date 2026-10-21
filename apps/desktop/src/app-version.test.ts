import { describe, expect, it, vi } from "vitest";

import { appInfo } from "~/lib/ipc";

import { loadVersion } from "./app-version";

vi.mock("~/lib/ipc");

describe("loadVersion", () => {
  it("returns the version from the app_info command", async () => {
    vi.mocked(appInfo).mockResolvedValueOnce({
      version: "1.2.3",
      logDir: "/logs",
    });
    await expect(loadVersion()).resolves.toBe("1.2.3");
  });

  it("returns an empty string when the command fails", async () => {
    vi.mocked(appInfo).mockRejectedValueOnce({
      kind: "internal",
      message: "boom",
    });
    await expect(loadVersion()).resolves.toBe("");
  });
});
