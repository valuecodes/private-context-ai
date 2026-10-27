# Security

## Architecture

```text
Webview (React UI)            no network: CSP default-src 'none', no Tauri plugins
    │
    │  Tauri IPC: commands, streamed token events          ── trust boundary 1
    ▼
Rust core                     commands, Ollama client, audit log, settings
    │    │
    │    └── SQLCipher database (chats, audit, settings) ◄── key from the OS keychain
    │
    │  plain HTTP to loopback only, no proxy, no redirects  ── trust boundary 2
    ▼
Ollama on 127.0.0.1:11434     a separate program you install and run
    │
    ▼  Ollama's own traffic (model pulls, updates), not the app's
Internet
```

- **Webview ↔ Rust.** The React frontend has no network access of its own. It calls
  Rust commands over Tauri IPC and receives streamed tokens as IPC events. Only
  `apps/desktop/src/lib/ipc.ts` imports `@tauri-apps/api`.
- **Rust ↔ Ollama.** Only Rust talks to Ollama: `/api/version`, `/api/tags`,
  `/api/show` (a model's capabilities, looked up once per model) and a streamed
  `/api/chat`, over plain HTTP to a loopback address.
- **Rust ↔ disk.** Chats, the audit log, settings and project files live in one
  SQLCipher database. Its key comes from the OS keychain and stays in Rust. Logs go to
  a separate folder and hold metadata only. Project files reach Rust as text: the
  webview reads only the files you pick or drop, with the HTML File API, and no Rust
  command takes or reads a disk path.

## Security model

What the app guarantees, and what enforces it:

- **The webview has no network.** The production CSP is `default-src 'none'` with
  `connect-src` limited to Tauri IPC. The app uses no Tauri plugins (no `http`, `fs`,
  `shell`, `opener` or `dialog`), grants the window two core permissions only
  (`capabilities/main.json`), and turns off `withGlobalTauri` and the asset protocol.
  The CSP binds the page, not the browser engine that renders it. On Windows the
  WebView2 runtime is started with switches (`additionalBrowserArgs` in
  `tauri.conf.json`) that turn off its crash reporting, component updates, metrics
  upload and background networking, on top of Tauri's own SmartScreen switch. Those
  switches are Chromium flags Microsoft does not guarantee, so they reduce the runtime's
  own traffic; they are not a promise that it stops (see Threat notes).
- **Rust reaches only Ollama on loopback.** The base URL, including one saved in
  Settings, must be plain `http` on `127.0.0.0/8`, `::1` or `localhost`, with no
  credentials, path, query or fragment. The HTTP client ignores system proxies, follows
  no redirects and pins `localhost` to `127.0.0.1`. `reqwest` is built without any TLS
  backend, so the binary cannot speak HTTPS at all.
- **The model gets one display-only tool, only when you turn Charts on, never with
  `format`.** The request type has no `format` field. `tools` is only ever the fixed
  `render_chart` definition in Rust, sent on the first request of a turn when the chat's
  Charts switch is on and the model's audited `/api/show` lists `tools`. Rust validates
  each call (bar, line or pie; 1 to 100 finite values; short trimmed text), and the
  webview draws it as SVG from those numbers: nothing runs, nothing is fetched, the model
  supplies no markup, and its text reaches the page only as text. A follow-up request with
  a fixed result line and no `tools` lets the model comment, so it cannot call again.
  Each request writes its own audit row. `think` is sent only as a boolean or a level the
  model advertises, and only to models whose audited `/api/show` lists `thinking`; it
  is `false` unless you turn Think on (models that always think get a level). Thinking
  text is stored encrypted, shown collapsed, and never sent back to the model.
  Comparing models adds only `keep_alive: 0`, which unloads each model but the last.
  Replies are rendered as Markdown into
  React elements with raw HTML disabled: links and images are inert (no `href`, nothing
  fetched), code highlighting is local JS, and nothing in a reply is executed.
- **Project memory is written by the model, and read as notes.** A project chat's
  summary is made by a separate `/api/chat` request with the chat's own model: no
  `tools`, no `format`, thinking off. It is audited as `ollama.memory` (Memory summary
  in Activity). Summaries are stored encrypted and sent to the project's other chats
  inside one delimited, escaped block that says they are reference notes, not
  instructions. That lowers the risk of a chat steering another through its summary,
  but cannot rule it out with an LLM. Because the model has no tools, the worst case is
  a worse answer. Every entry can be viewed, edited and deleted, and "View full memory"
  shows the exact message sent. A change that gives the model tools must revisit this.
  Summaries also run in the background: about a minute after a project chat's last
  reply, and to catch up on unsaved chats shortly after startup and when a project is
  opened. They run one at a time, only while no other request is in flight, and any new
  request cancels them. Each is audited like a manual refresh, and "Automatic memory"
  turns them off per project ("Refresh memory" still works).
- **Project files are searched locally and read as data.** In a project with files,
  Rust searches the encrypted full-text index for the prompt of each turn and adds up
  to six of the best passages to that turn's one `system` message, after project
  memory, within a byte budget that never takes the request over its size limit. This
  adds no request: each message is still one audited `ollama.chat` request. Each
  passage sits inside one delimited, escaped block, named by file and lines, under a
  line saying it is reference data, not instructions. Like memory, that lowers the
  risk of a file steering the model but cannot rule it out. The chat's Files switch
  (on by default, shown in projects with files) turns this off, and "What this chat
  sees" shows the exact passages sending the current draft would add.
- **History is encrypted at rest.** Everything is in a SQLCipher database whose random
  32-byte key lives in the OS keychain and never reaches the webview. Without a working
  keychain the app stores nothing. Temporary chats are kept only in process memory,
  never written to disk; audit metadata is still recorded.
- **Every request is recorded.** Each outbound request writes an `attempt` row to the
  encrypted audit log before its socket opens, then its outcome. If the log cannot be
  written, the request is refused. The Activity view shows these rows.
- **Logs carry no content.** Log lines hold kinds, hosts, counts, durations and status.
  Prompt and reply text travel in a `Redacted` type that prints as
  `[redacted, N chars]`.

## Threat notes

What this does **not** protect against:

- **Ollama itself.** Ollama is a separate program with its own network access: it
  downloads models (`ollama pull`), may check for updates, and listens on loopback,
  where any local program can talk to it. The app's guarantees end at Ollama's port.
  Everything you send is in Ollama's memory, and with `OLLAMA_DEBUG=1` Ollama writes
  full prompts to its own `server.log`.
- **The logged-in user's session.** Malware running as you can read the keychain entry
  and so the database. Encryption at rest protects a copied disk or a lost laptop that
  is powered off, not a compromised session.
- **Your OS account protects the key.** The keychain entry is sealed with your login
  (DPAPI on Windows). Someone with the disk and your account password, or a weak one
  they can crack offline, can unseal the key and open the database. A strong password or
  full-disk encryption (BitLocker, FileVault) is what makes a lost laptop safe. With a
  roaming profile, both the encrypted database and its key roam with it.
- **The clipboard.** Copy as Markdown and the copy buttons put plain text on the
  clipboard. Windows Clipboard history keeps it (Win+V) and "Sync across your devices"
  uploads it; clipboard managers keep it too. A webview cannot mark clipboard text as
  excluded from those, so turn them off before copying sensitive chats.
- **Memory on disk.** Chat text and key material in the app's process memory can end up
  in the page file, the hibernation file or a crash dump (Windows Error Reporting local
  dumps, when an administrator has turned them on). Full-disk encryption covers them.
- **The browser engine's own traffic.** On Windows the WebView2 runtime (Edge) fetches
  components and reports diagnostics on its own, outside the page and its CSP. That
  traffic carries no chat text in normal operation, but a renderer crash report would
  include page memory. The switches above reduce it; Windows diagnostic data settings
  and Microsoft's own policy decide the rest.
- **The screen.** Screenshots, screen recording, remote desktop and Windows Recall see
  whatever the window shows.
- **Project memory, on purpose.** With Auto memory on, a summary of each chat in a
  project is sent to the project's other chats, temporary ones included. Keep a chat out
  of a project, or turn Auto memory off, when its summary must not travel.
- **Project files, on purpose.** A file added to a project can reach every chat in it,
  temporary ones included, as passages in the system message. A file's text can try to
  steer the model like any pasted text. Add only files whose text may go to the
  project's chats, or turn Files off in a chat.
- **What the model says.** Replies can be wrong, made up or harmful. The model cannot
  act, but you can be misled.
- **Debug builds.** A development build reads `PRIVATE_CONTEXT_OLLAMA_URL` to override
  the base URL; it is still loopback-guarded. Release builds ignore it.

## Future work

Not built yet; the layout leaves room for it:

- **Redaction and pseudonymization.** A Rust `pipeline` step between the chat command
  and the Ollama client, a `documents` table and a pseudonym map in the same SQLCipher
  database, and a new feature folder in the UI.
- **Passphrase unlock.** A passphrase-based `KeySource` (Argon2id, wrapped key) for
  systems without a keychain.
