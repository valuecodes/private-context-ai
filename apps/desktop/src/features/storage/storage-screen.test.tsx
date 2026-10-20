import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { Problem } from "./storage-screen";
import { StorageScreen } from "./storage-screen";

const render = (status: Problem): string =>
  renderToString(<StorageScreen status={status} />);

describe("StorageScreen", () => {
  it("explains a missing key without offering a new one", () => {
    const html = render({
      state: "keyMissing",
      dbPath: "/data/private-context.db",
    });
    expect(html).toContain("Secure storage unavailable");
    expect(html).toContain("/data/private-context.db");
    expect(html).toContain("A new key is never created");
    expect(html).toContain("db-key-v1");
  });

  it("explains an unavailable keychain with its reason", () => {
    const html = render({
      state: "keychainUnavailable",
      detail: "no keychain is available on this system",
    });
    expect(html).toContain("Secure storage unavailable");
    expect(html).toContain("no keychain is available on this system");
    expect(html).toContain("Secret Service");
  });

  it("explains a wrong key", () => {
    const html = render({
      state: "wrongKey",
      dbPath: "/data/private-context.db",
    });
    expect(html).toContain("does not open the chat database");
  });

  it("explains an I/O failure", () => {
    expect(render({ state: "io", detail: "disk full" })).toContain(
      "could not be opened: disk full"
    );
  });

  it("says another window is already running", () => {
    const html = render({ state: "alreadyRunning" });
    expect(html).toContain("already running");
    expect(html).not.toContain("Secure storage unavailable");
  });

  it("is announced as an alert", () => {
    expect(render({ state: "alreadyRunning" })).toContain('role="alert"');
  });
});
