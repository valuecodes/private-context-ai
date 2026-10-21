import { afterEach, describe, expect, it, vi } from "vitest";

import { copyText } from "./clipboard";

const fakeDocument = (copied: boolean) => {
  const textarea = {
    value: "",
    className: "",
    setAttribute: vi.fn(),
    select: vi.fn(),
    remove: vi.fn(),
  };
  return {
    textarea,
    document: {
      createElement: vi.fn(() => textarea),
      body: { append: vi.fn() },
      execCommand: vi.fn(() => copied),
    },
  };
};

describe("copyText", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("resolves true when the clipboard API writes", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await expect(copyText("hi")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hi");
  });

  it("falls back to execCommand when the clipboard API rejects", async () => {
    vi.stubGlobal("navigator", {
      clipboard: { writeText: () => Promise.reject(new Error("denied")) },
    });
    const fake = fakeDocument(true);
    vi.stubGlobal("document", fake.document);
    await expect(copyText("hi")).resolves.toBe(true);
    expect(fake.textarea.value).toBe("hi");
    expect(fake.document.execCommand).toHaveBeenCalledWith("copy");
    expect(fake.textarea.remove).toHaveBeenCalled();
  });

  it("resolves false when the fallback cannot copy either", async () => {
    vi.stubGlobal("navigator", {});
    vi.stubGlobal("document", fakeDocument(false).document);
    await expect(copyText("hi")).resolves.toBe(false);
  });

  it("resolves false without a clipboard API or a document", async () => {
    vi.stubGlobal("navigator", {});
    await expect(copyText("hi")).resolves.toBe(false);
  });
});
