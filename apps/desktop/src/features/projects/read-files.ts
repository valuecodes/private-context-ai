// Reading the files the user picks or drops, in the webview. Rust gets their
// text only; it has no command that reads a disk path.

/** The largest file Rust stores, in bytes. */
const MAX_FILE_BYTES = 4 * 1024 * 1024;

/** What reading a file needs: a `File` has both. */
type Readable = Pick<Blob, "size" | "arrayBuffer">;

/** A file to add, named by its path inside the folder it came with. */
type PickedFile = { name: string; file: Readable };

type PreparedFile =
  | { kind: "ok"; name: string; content: string }
  | { kind: "skipped"; name: string; reason: string };

/**
 * A dropped file or folder. `FileSystemEntry` and its file and directory
 * kinds fit these shapes, and so do test fakes.
 */
type DropEntry = {
  readonly fullPath: string;
  readonly isFile: boolean;
  readonly isDirectory: boolean;
};
type DropFileEntry = DropEntry & {
  file: (
    success: (file: File) => void,
    error?: (error: DOMException) => void
  ) => void;
};
type DropDirectoryEntry = DropEntry & {
  createReader: () => {
    readEntries: (
      success: (entries: DropEntry[]) => void,
      error?: (error: DOMException) => void
    ) => void;
  };
};

const isFileEntry = (entry: DropEntry): entry is DropFileEntry =>
  entry.isFile && "file" in entry;

const isDirectoryEntry = (entry: DropEntry): entry is DropDirectoryEntry =>
  entry.isDirectory && "createReader" in entry;

// `fatal` makes a byte sequence that is not UTF-8 an error, where the default
// would replace it with U+FFFD, which is also valid text on its own.
const decoder = new TextDecoder("utf-8", { fatal: true });

/** A picked file's path inside its folder, or its name when picked alone. */
const fileName = (file: Pick<File, "name" | "webkitRelativePath">): string =>
  file.webkitRelativePath === "" ? file.name : file.webkitRelativePath;

/** The file's text, or why it is skipped: too large, not text, unreadable. */
const prepareFile = async ({
  name,
  file,
}: PickedFile): Promise<PreparedFile> => {
  if (file.size > MAX_FILE_BYTES) {
    return { kind: "skipped", name, reason: "larger than 4 MiB" };
  }
  let bytes: ArrayBuffer;
  try {
    bytes = await file.arrayBuffer();
  } catch {
    return { kind: "skipped", name, reason: "could not be read" };
  }
  let content: string;
  try {
    content = decoder.decode(bytes);
  } catch {
    return { kind: "skipped", name, reason: "not UTF-8 text" };
  }
  if (content.includes("\0")) {
    return { kind: "skipped", name, reason: "a binary file" };
  }
  return { kind: "ok", name, content };
};

/** Every entry of a folder: `readEntries` returns them in batches. */
const readAll = (entry: DropDirectoryEntry): Promise<DropEntry[]> =>
  new Promise((resolve, reject) => {
    const reader = entry.createReader();
    const all: DropEntry[] = [];
    const next = () => {
      reader.readEntries((batch) => {
        if (batch.length === 0) {
          resolve(all);
        } else {
          all.push(...batch);
          next();
        }
      }, reject);
    };
    next();
  });

const entryFile = (entry: DropFileEntry): Promise<File> =>
  new Promise((resolve, reject) => {
    entry.file(resolve, reject);
  });

/**
 * The files of dropped entries, folders walked in full, each named by its
 * path without the leading `/`. Entries are read one at a time, in order.
 */
const collectDropped = async (entries: DropEntry[]): Promise<PickedFile[]> => {
  const picked: PickedFile[] = [];
  const queue = [...entries];
  for (let entry = queue.shift(); entry !== undefined; entry = queue.shift()) {
    const name = entry.fullPath.replace(/^\//, "");
    if (isFileEntry(entry)) {
      picked.push({ name, file: await entryFile(entry) });
    } else if (isDirectoryEntry(entry)) {
      queue.unshift(...(await readAll(entry)));
    }
  }
  return picked;
};

export type { DropEntry, PickedFile };
export { collectDropped, fileName, MAX_FILE_BYTES, prepareFile };
