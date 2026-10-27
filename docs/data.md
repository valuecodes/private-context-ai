# Where is my data

A temporary chat (the Temporary button on an empty new chat) is never written here: it
lives only in the app's memory and is gone once you leave it or quit. Its requests
still show in Activity.

Conversations live in one encrypted SQLite database (SQLCipher), `private-context.db`,
in the app data folder. While the app runs, `private-context.db-wal` and
`private-context.db-shm` sit next to it, along with `private-context.lock`, which
keeps a second copy of the app from opening the database.

| OS      | App data folder                                                       |
| ------- | --------------------------------------------------------------------- |
| Windows | `%APPDATA%\io.github.valuecodes.privatecontextai`                     |
| macOS   | `~/Library/Application Support/io.github.valuecodes.privatecontextai` |
| Linux   | `~/.local/share/io.github.valuecodes.privatecontextai`                |

The database key is 32 random bytes created on first start and kept only in the OS
keychain, under service `private-context-ai` and account `db-key-v1`. On Windows that
is the generic credential `db-key-v1.private-context-ai` in Credential Manager; on
macOS, the login Keychain; on Linux, the Secret Service (GNOME Keyring, KWallet). The
key never reaches the app window, and without a working keychain the app shows
"Secure storage unavailable" instead of storing anything unencrypted.

**Project files.** Files you add to a project are copies: their text is stored in the
same encrypted database, with a full-text index, and the originals are never read
again. To update one, add it again; a file of the same name is replaced. Deleting a
file, or its project, deletes the copy and its index entries. Each chat in the project
searches them for every message it sends, and puts the best passages in its request
to the local model, unless the chat's Files switch is off (saved with the chat).

**Logs.** Daily JSON log files (`app.*.log`, the last 14 kept) are written to the app
log folder, shown in Settings: `%LOCALAPPDATA%\io.github.valuecodes.privatecontextai\logs`
on Windows, `~/Library/Logs/io.github.valuecodes.privatecontextai` on macOS and
`~/.local/share/io.github.valuecodes.privatecontextai/logs` on Linux. They are not
encrypted and hold only metadata: request kinds, hosts, token counts, durations and
status, never message text.

**Key missing.** If the database exists but its keychain entry is gone, the app says
so and stops. It never creates a new key, since that would make the existing chats
unreadable for good. Restore the entry if you can.

**Wiping everything.** Quit the app, delete `private-context.db` and its `-wal` and
`-shm` files, then delete the keychain entry. The next start creates a new key and an
empty database.
