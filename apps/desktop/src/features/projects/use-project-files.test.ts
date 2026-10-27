import { describe, expect, it, vi } from "vitest";

import type { ProjectFile } from "~/lib/types";

import type { FilesState } from "./use-project-files";
import { filesReducer, initialFiles, isBusy } from "./use-project-files";

vi.mock("~/lib/ipc");

const file = (id: string): ProjectFile => ({
  id,
  name: `${id}.md`,
  bytes: 10,
  lineCount: 2,
  createdAt: "x",
});

const ids = (state: FilesState): string[] => state.files.map(({ id }) => id);

const loaded = (files: ProjectFile[], total: number): FilesState =>
  filesReducer(initialFiles, {
    type: "loaded",
    page: { files, total },
    append: false,
  });

describe("filesReducer", () => {
  it("starts loading with nothing", () => {
    expect(initialFiles.phase).toBe("loading");
    expect(isBusy(initialFiles)).toBe(false);
  });

  it("appends the next page and replaces on a first page", () => {
    const first = loaded([file("a"), file("b")], 3);
    expect(first.phase).toBe("ready");
    const next = filesReducer(first, {
      type: "loaded",
      page: { files: [file("c")], total: 3 },
      append: true,
    });
    expect(ids(next)).toEqual(["a", "b", "c"]);

    const reloaded = filesReducer(next, {
      type: "loaded",
      page: { files: [file("z")], total: 1 },
      append: false,
    });
    expect(ids(reloaded)).toEqual(["z"]);
    expect(reloaded.total).toBe(1);
  });

  it("counts a delete in the total, so the next page offset holds", () => {
    const state = loaded([file("a"), file("b")], 5);
    const deleting = filesReducer(state, { type: "deleting", id: "a" });
    expect(isBusy(deleting)).toBe(true);

    const deleted = filesReducer(deleting, { type: "deleted", id: "a" });
    expect(ids(deleted)).toEqual(["b"]);
    expect(deleted.total).toBe(4);
    expect(isBusy(deleted)).toBe(false);

    const again = filesReducer(deleted, { type: "deleted", id: "a" });
    expect(again.total).toBe(4);
  });

  it("tracks an add's progress and skipped files until it ends", () => {
    const state = filesReducer(
      { ...loaded([], 0), skipped: [{ name: "old", reason: "r" }] },
      { type: "adding", of: 3 }
    );
    expect(state.adding).toEqual({ done: 0, of: 3 });
    expect(state.skipped).toEqual([]);
    expect(isBusy(state)).toBe(true);

    const skipped = filesReducer(
      filesReducer(state, { type: "progress", done: 1 }),
      { type: "skipped", file: { name: "x.png", reason: "a binary file" } }
    );
    expect(skipped.adding).toEqual({ done: 1, of: 3 });
    expect(skipped.skipped).toEqual([
      { name: "x.png", reason: "a binary file" },
    ]);

    const done = filesReducer(skipped, { type: "added" });
    expect(done.adding).toBeNull();
    expect(done.skipped).toHaveLength(1);
    expect(isBusy(done)).toBe(false);
  });

  it("ignores progress outside an add", () => {
    const state = loaded([], 0);
    expect(filesReducer(state, { type: "progress", done: 2 })).toBe(state);
  });

  it("keeps a failure and ends a delete", () => {
    const deleting = filesReducer(loaded([file("a")], 1), {
      type: "deleting",
      id: "a",
    });
    const failed = filesReducer(deleting, {
      type: "failed",
      error: { kind: "database", message: "disk full" },
    });
    expect(failed.error?.message).toBe("disk full");
    expect(failed.deleting).toBeNull();
    expect(ids(failed)).toEqual(["a"]);
  });
});
