import { describe, expect, it } from "vitest";

import type { DropEntry } from "./read-files";
import {
  collectDropped,
  fileName,
  MAX_FILE_BYTES,
  prepareFile,
} from "./read-files";

const blob = (...parts: BlobPart[]) => new Blob(parts);

describe("fileName", () => {
  it("uses the path inside a picked folder, else the name", () => {
    expect(fileName({ name: "a.md", webkitRelativePath: "" })).toBe("a.md");
    expect(
      fileName({ name: "a.md", webkitRelativePath: "notes/2026/a.md" })
    ).toBe("notes/2026/a.md");
  });
});

describe("prepareFile", () => {
  it("reads UTF-8 text, a literal U+FFFD included", async () => {
    expect(
      await prepareFile({ name: "a.md", file: blob("päätös � ok") })
    ).toEqual({ kind: "ok", name: "a.md", content: "päätös � ok" });
    expect(await prepareFile({ name: "empty", file: blob() })).toEqual({
      kind: "ok",
      name: "empty",
      content: "",
    });
  });

  it("skips bytes that are not UTF-8", async () => {
    const latin1 = blob(new Uint8Array([0x70, 0xe4, 0xe4, 0x74]));
    expect(await prepareFile({ name: "old.txt", file: latin1 })).toEqual({
      kind: "skipped",
      name: "old.txt",
      reason: "not UTF-8 text",
    });
  });

  it("skips binary files", async () => {
    // Valid UTF-8, as many binary formats' first bytes are, but with NUL.
    const zip = blob(new Uint8Array([0x50, 0x4b, 0x03, 0x04, 0x00, 0x00]));
    expect(await prepareFile({ name: "x.zip", file: zip })).toMatchObject({
      kind: "skipped",
      reason: "a binary file",
    });
  });

  it("skips files over 4 MiB without reading them", async () => {
    const huge = {
      size: MAX_FILE_BYTES + 1,
      arrayBuffer: () => Promise.reject(new Error("read")),
    };
    expect(await prepareFile({ name: "big.log", file: huge })).toMatchObject({
      kind: "skipped",
      reason: "larger than 4 MiB",
    });
    const atLimit = blob("x".repeat(MAX_FILE_BYTES));
    expect((await prepareFile({ name: "ok.log", file: atLimit })).kind).toBe(
      "ok"
    );
  });

  it("skips a file that cannot be read", async () => {
    const gone = {
      size: 1,
      arrayBuffer: () => Promise.reject(new Error("gone")),
    };
    expect(await prepareFile({ name: "gone", file: gone })).toMatchObject({
      kind: "skipped",
      reason: "could not be read",
    });
  });
});

const fileEntry = (fullPath: string, text = fullPath) => ({
  fullPath,
  isFile: true,
  isDirectory: false,
  file: (success: (file: File) => void) => {
    success(new File([text], fullPath.split("/").at(-1) ?? ""));
  },
});

/** A folder whose reader returns `batches` one call at a time, then none. */
const folderEntry = (fullPath: string, batches: DropEntry[][]) => ({
  fullPath,
  isFile: false,
  isDirectory: true,
  createReader: () => {
    const left = [...batches];
    return {
      readEntries: (success: (entries: DropEntry[]) => void) => {
        success(left.shift() ?? []);
      },
    };
  },
});

describe("collectDropped", () => {
  it("walks folders in order and names files by their path", async () => {
    const dropped = await collectDropped([
      fileEntry("/top.md"),
      folderEntry("/notes", [
        [
          fileEntry("/notes/a.md"),
          folderEntry("/notes/sub", [[fileEntry("/notes/sub/b.md")]]),
        ],
        [fileEntry("/notes/c.md")],
      ]),
      folderEntry("/empty", []),
    ]);

    expect(dropped.map(({ name }) => name)).toEqual([
      "top.md",
      "notes/a.md",
      "notes/sub/b.md",
      "notes/c.md",
    ]);
    expect(await dropped[1]?.file.arrayBuffer()).toEqual(
      await blob("/notes/a.md").arrayBuffer()
    );
  });

  it("keeps going past a folder or file that cannot be opened", async () => {
    const broken = {
      fullPath: "/broken",
      isFile: false,
      isDirectory: true,
      createReader: () => ({
        readEntries: (
          _success: (entries: DropEntry[]) => void,
          error?: (error: DOMException) => void
        ) => {
          error?.(new DOMException("denied"));
        },
      }),
    };
    const gone = {
      fullPath: "/gone.md",
      isFile: true,
      isDirectory: false,
      file: (
        _success: (file: File) => void,
        error?: (error: DOMException) => void
      ) => {
        error?.(new DOMException("not found"));
      },
    };

    const dropped = await collectDropped([broken, gone, fileEntry("/ok.md")]);

    expect(dropped.map(({ name }) => name)).toEqual([
      "broken",
      "gone.md",
      "ok.md",
    ]);
    const prepared = await Promise.all(dropped.map(prepareFile));
    expect(prepared.map((file) => file.kind)).toEqual([
      "skipped",
      "skipped",
      "ok",
    ]);
    expect(prepared[0]).toMatchObject({ reason: "could not be read" });
  });
});
