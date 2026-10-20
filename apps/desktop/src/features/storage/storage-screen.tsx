import type { StorageStatus } from "~/lib/types";

type Problem = Exclude<StorageStatus, { state: "ok" }>;

type Explanation = {
  title: string;
  reason: string;
  steps: string[];
};

const WIPE =
  "To start over and lose all saved chats, quit the app, delete the database file (and its -wal and -shm files), then delete the keychain entry “private-context-ai” / “db-key-v1”.";

const UNAVAILABLE = "Secure storage unavailable";

const explain = (status: Problem): Explanation => {
  if (status.state === "alreadyRunning") {
    return {
      title: "Private Context AI is already running",
      reason:
        "Another window of the app has the chat history open. Only one can use it at a time.",
      steps: ["Close this window and switch to the one that is open."],
    };
  }
  if (status.state === "keychainUnavailable") {
    return {
      title: UNAVAILABLE,
      reason: `The OS keychain could not be used (${status.detail}), so there is nowhere safe to keep the database key. Chats are never stored unencrypted instead.`,
      steps: [
        "Windows uses Credential Manager and macOS uses the Keychain; on Linux a Secret Service (such as GNOME Keyring or KWallet) must be running and unlocked.",
        "Fix that, then restart the app.",
      ],
    };
  }
  if (status.state === "keyMissing") {
    return {
      title: UNAVAILABLE,
      reason: `The chat database exists at ${status.dbPath}, but its key is missing from the keychain, so it cannot be read. A new key is never created, because that would make the existing chats unreadable for good.`,
      steps: [
        "If you restored the keychain from a backup, or the entry is in another keychain, put it back and restart the app.",
        WIPE,
      ],
    };
  }
  if (status.state === "wrongKey") {
    return {
      title: UNAVAILABLE,
      reason: `The key in the keychain does not open the chat database at ${status.dbPath}.`,
      steps: [
        "If the keychain entry was replaced, restore the previous one and restart the app.",
        WIPE,
      ],
    };
  }
  return {
    title: UNAVAILABLE,
    reason: `The chat database could not be opened: ${status.detail}`,
    steps: [
      "Check that the app data folder is writable and the disk is not full, then restart the app.",
    ],
  };
};

/** Full-screen explanation shown instead of the app when storage is closed. */
const StorageScreen = ({ status }: { status: Problem }) => {
  const { title, reason, steps } = explain(status);
  return (
    <main className="flex h-screen items-center justify-center bg-zinc-50 p-8 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      <div role="alert" className="max-w-xl">
        <h1 className="text-lg font-semibold">{title}</h1>
        <p className="mt-3 text-sm">{reason}</p>
        <ul className="mt-3 list-disc pl-5 text-sm text-zinc-600 dark:text-zinc-400">
          {steps.map((step) => (
            <li key={step} className="mt-1">
              {step}
            </li>
          ))}
        </ul>
      </div>
    </main>
  );
};

export type { Problem };
export { StorageScreen };
