import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { ProjectFile } from "~/lib/types";

import { FilesList, formatBytes } from "./files-panel";
import type { FilesState } from "./use-project-files";
import { initialFiles } from "./use-project-files";

vi.mock("~/lib/ipc");

const file = (
  id: string,
  overrides: Partial<ProjectFile> = {}
): ProjectFile => ({
  id,
  name: `${id}.md`,
  bytes: 2048,
  lineCount: 12,
  createdAt: "x",
  ...overrides,
});

const list = (state: Partial<FilesState>) =>
  renderToString(
    <FilesList
      state={{ ...initialFiles, phase: "ready", ...state }}
      onAdd={vi.fn()}
      onDrop={vi.fn()}
      onMore={vi.fn()}
      onRemove={vi.fn()}
    />
  );

/** How many controls render disabled. */
const disabledCount = (html: string): number =>
  html.match(/disabled=""/g)?.length ?? 0;

describe("formatBytes", () => {
  it("picks B, KiB or MiB", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatBytes(4 * 1024 * 1024)).toBe("4.0 MiB");
  });
});

describe("FilesList", () => {
  it("says when there are no files", () => {
    const html = list({});
    expect(html).toContain("No files in this project yet");
    expect(html).toContain("Add files");
    expect(html).toContain("Add folder");
    expect(disabledCount(html)).toBe(0);
  });

  it("says nothing about files while loading", () => {
    expect(list({ phase: "loading" })).not.toContain("No files");
  });

  it("lists files with size and line count", () => {
    const html = list({
      files: [
        file("a"),
        file("b", { bytes: 10, lineCount: 1, name: "dir/b.txt" }),
      ],
      total: 2,
    });
    expect(html).toContain("Files (2)");
    expect(html).toContain("a.md");
    expect(html).toContain("2.0 KiB · 12 lines");
    expect(html).toContain("dir/b.txt");
    expect(html).toContain("10 B · 1 line");
    expect(html).toContain('aria-label="Delete dir/b.txt"');
    expect(html).not.toContain("Show more");
  });

  it("offers more when not every file is loaded", () => {
    expect(list({ files: [file("a")], total: 201 })).toContain(
      "Show more (200 left)"
    );
  });

  it("shows progress and skipped files with why", () => {
    const html = list({
      adding: { done: 3, of: 10 },
      skipped: [
        { name: "x.png", reason: "a binary file" },
        { name: "big.log", reason: "larger than 4 MiB" },
      ],
    });
    expect(html).toContain("Adding 3 of 10…");
    expect(html).toContain("Skipped 2 files:");
    expect(html).toContain("x.png: a binary file");
    expect(html).toContain("big.log: larger than 4 MiB");
  });

  it("disables adding, deleting and more while busy", () => {
    const files = [file("a"), file("b")];
    // Two file inputs, two Delete buttons and Show more.
    expect(
      disabledCount(list({ files, total: 3, adding: { done: 0, of: 1 } }))
    ).toBe(5);
    expect(disabledCount(list({ files, total: 3, deleting: "a" }))).toBe(5);
    expect(disabledCount(list({ files, total: 3 }))).toBe(0);
  });

  it("shows an error", () => {
    expect(
      list({ error: { kind: "database", message: "disk full" } })
    ).toContain("disk full");
  });
});
