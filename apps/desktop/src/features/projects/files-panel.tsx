import { FilePlus, FolderPlus, Trash } from "lucide-react";
import type { ChangeEvent, DragEvent } from "react";

import type { DropEntry, PickedFile } from "./read-files";
import { fileName } from "./read-files";
import type { FilesState } from "./use-project-files";
import { isBusy, useProjectFiles } from "./use-project-files";

const button =
  "flex cursor-pointer items-center gap-1 rounded border border-zinc-300 px-2 py-1 text-sm hover:bg-zinc-100 has-disabled:cursor-not-allowed has-disabled:opacity-40 disabled:cursor-not-allowed disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";
const small =
  "flex items-center gap-1 rounded px-1.5 py-0.5 text-xs hover:bg-zinc-100 disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-zinc-800";

/** `bytes` in B, KiB or MiB. */
const formatBytes = (bytes: number): string => {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
};

const formatLines = (lines: number): string =>
  `${lines} ${lines === 1 ? "line" : "lines"}`;

/** The picked files, named by their path inside a picked folder. */
const pickedFiles = (event: ChangeEvent<HTMLInputElement>): PickedFile[] => {
  const files = [...(event.currentTarget.files ?? [])];
  // Picking the same files again fires `change` again.
  event.currentTarget.value = "";
  return files.map((file) => ({ name: fileName(file), file }));
};

/** A drop's files and folders; read now, while the drop event lasts. */
const droppedEntries = (event: DragEvent<HTMLElement>): DropEntry[] =>
  [...event.dataTransfer.items]
    .map((item) => item.webkitGetAsEntry())
    .filter((entry) => entry !== null);

type FilesListProps = {
  state: FilesState;
  onAdd: (files: PickedFile[]) => void;
  onDrop: (entries: DropEntry[]) => void;
  onMore: () => void;
  onRemove: (id: string) => void;
};

/** The files section of a project: adding, listing and deleting files. */
const FilesList = ({
  state,
  onAdd,
  onDrop,
  onMore,
  onRemove,
}: FilesListProps) => {
  const busy = isBusy(state);
  return (
    <section aria-labelledby="project-files" className="flex flex-col gap-2">
      <h3 id="project-files" className="font-medium">
        {state.total === 0 ? "Files" : `Files (${state.total})`}
      </h3>
      <div
        onDragOver={(event) => {
          event.preventDefault();
        }}
        onDrop={(event) => {
          event.preventDefault();
          if (!busy) {
            onDrop(droppedEntries(event));
          }
        }}
        className="flex flex-wrap items-center gap-2 rounded border border-dashed border-zinc-300 p-3 dark:border-zinc-700"
      >
        <label className={button}>
          <FilePlus aria-hidden="true" className="size-4" />
          Add files
          <input
            type="file"
            multiple
            disabled={busy}
            onChange={(event) => {
              onAdd(pickedFiles(event));
            }}
            className="sr-only"
          />
        </label>
        <label className={button}>
          <FolderPlus aria-hidden="true" className="size-4" />
          Add folder
          <input
            type="file"
            multiple
            disabled={busy}
            // Not in React's types: it makes the picker choose a folder.
            ref={(element) => {
              element?.setAttribute("webkitdirectory", "");
            }}
            onChange={(event) => {
              onAdd(pickedFiles(event));
            }}
            className="sr-only"
          />
        </label>
        <span className="text-sm text-zinc-500 dark:text-zinc-400">
          or drop files and folders here
        </span>
      </div>
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        Copies of text files, up to 4 MiB each, and the text of PDFs up to 32
        MiB (the first 4 MiB of their text), stored encrypted on this computer.
      </p>
      {state.adding !== null && (
        <p role="status" className="text-sm">
          {state.adding.of === 0
            ? "Reading the dropped files…"
            : `Adding ${state.adding.done} of ${state.adding.of}…`}
        </p>
      )}
      {state.error !== null && (
        <p role="alert" className="text-sm text-red-600 dark:text-red-400">
          {state.error.message}
        </p>
      )}
      {state.skipped.length > 0 && (
        <div className="text-sm">
          <p>{`Skipped ${state.skipped.length} ${state.skipped.length === 1 ? "file" : "files"}:`}</p>
          <ul aria-label="Skipped files" className="flex flex-col gap-0.5">
            {/* One batch never names a file twice: names are paths. */}
            {state.skipped.map((file) => (
              <li key={file.name} className="text-zinc-600 dark:text-zinc-400">
                {`${file.name}: ${file.reason}`}
              </li>
            ))}
          </ul>
        </div>
      )}
      {state.phase === "ready" && state.total === 0 && (
        <p className="text-sm text-zinc-500 dark:text-zinc-400">
          No files in this project yet
        </p>
      )}
      {state.files.length > 0 && (
        <ul aria-label="Project files" className="flex flex-col gap-0.5">
          {state.files.map((file) => (
            <li key={file.id} className="flex items-center gap-2 text-sm">
              <span className="min-w-0 flex-1 truncate" title={file.name}>
                {file.name}
              </span>
              <span className="text-xs text-zinc-500 tabular-nums dark:text-zinc-400">
                {`${formatBytes(file.bytes)} · ${formatLines(file.lineCount)}`}
              </span>
              <button
                type="button"
                onClick={() => {
                  onRemove(file.id);
                }}
                disabled={busy}
                aria-label={`Delete ${file.name}`}
                title="Delete"
                className={small}
              >
                <Trash aria-hidden="true" className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}
      {state.files.length < state.total && (
        <button
          type="button"
          onClick={onMore}
          disabled={busy}
          className={`${button} self-start`}
        >
          {`Show more (${state.total - state.files.length} left)`}
        </button>
      )}
    </section>
  );
};

/** The file library of one project. */
const FilesPanel = ({ projectId }: { projectId: string }) => {
  const files = useProjectFiles(projectId);
  return (
    <FilesList
      state={files.state}
      onAdd={files.add}
      onDrop={files.addDropped}
      onMore={files.more}
      onRemove={files.remove}
    />
  );
};

export { FilesList, FilesPanel, formatBytes };
