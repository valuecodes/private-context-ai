import { afterEach, describe, expect, it, vi } from "vitest";

import { copyLater, copyText } from "./clipboard";

const fakeDocument = (copied: boolean) => {
  const textarea = {
    value: "",
    className: "",
    setAttribute: vi.fn(),
    select: vi.fn(),
    remove: vi.fn(),
  };
  const button = { focus: vi.fn() };
  return {
    textarea,
    button,
    document: {
      activeElement: button,
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
    expect(fake.button.focus).toHaveBeenCalled();
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

/** A `ClipboardItem` that keeps what it was given. */
class FakeItem {
  constructor(readonly items: Record<string, Promise<Blob>>) {}
}

describe("copyLater", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("writes a ClipboardItem before the text has arrived", async () => {
    vi.stubGlobal("ClipboardItem", FakeItem);
    const write = vi.fn((_items: FakeItem[]) => Promise.resolve());
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal("navigator", { clipboard: { write, writeText } });
    const { promise: text, resolve } = Promise.withResolvers<string>();

    const copied = copyLater(text);
    // Synchronously, while the click still counts as user activation.
    expect(write).toHaveBeenCalledTimes(1);
    resolve("# Doc");
    await expect(copied).resolves.toBe(true);
    const blob = await write.mock.calls[0]?.[0][0]?.items["text/plain"];
    await expect(blob?.text()).resolves.toBe("# Doc");
    expect(writeText).not.toHaveBeenCalled();
  });

  it("falls back to writeText without ClipboardItem", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await expect(copyLater(Promise.resolve("# Doc"))).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("# Doc");
  });

  it("resolves false when the text cannot be fetched", async () => {
    vi.stubGlobal("ClipboardItem", FakeItem);
    const write = vi.fn(async (items: FakeItem[]) => {
      await items[0]?.items["text/plain"];
    });
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal("navigator", { clipboard: { write, writeText } });
    await expect(
      copyLater(Promise.reject(new Error("not found")))
    ).resolves.toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });
});
