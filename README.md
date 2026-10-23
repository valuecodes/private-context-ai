# Private Context AI

A secure local AI sandbox: a Tauri 2 desktop app that streams chat replies from an
Ollama server you run yourself, on this computer. Chat history is encrypted at rest, the
model gets at most one display-only tool (`render_chart`, only when you turn Charts on),
the app contacts nothing but Ollama on loopback, and an Activity view lists every request
it made.

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
- **Rust ↔ disk.** Chats, the audit log and settings live in one SQLCipher database.
  Its key comes from the OS keychain and stays in Rust. Logs go to a separate folder
  and hold metadata only.

## Security model

What the app guarantees, and what enforces it:

- **The webview has no network.** The production CSP is `default-src 'none'` with
  `connect-src` limited to Tauri IPC. The app uses no Tauri plugins (no `http`, `fs`,
  `shell`, `opener` or `dialog`), grants the window three core permissions only
  (`capabilities/main.json`), and turns off `withGlobalTauri` and the asset protocol.
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

### Threat notes

What this does **not** protect against:

- **Ollama itself.** Ollama is a separate program with its own network access: it
  downloads models (`ollama pull`), may check for updates, and listens on loopback,
  where any local program can talk to it. The app's guarantees end at Ollama's port.
- **The logged-in user's session.** Malware running as you can read the keychain entry
  and so the database. Encryption at rest protects a copied disk or a lost laptop that
  is powered off, not a compromised session.
- **What the model says.** Replies can be wrong, made up or harmful. The model cannot
  act, but you can be misled.
- **Debug builds.** A development build reads `PRIVATE_CONTEXT_OLLAMA_URL` to override
  the base URL; it is still loopback-guarded. Release builds ignore it.

### Future work

Not built yet; the layout leaves room for it:

- **Redaction and pseudonymization.** A Rust `pipeline` step between the chat command
  and the Ollama client, a `documents` table and a pseudonym map in the same SQLCipher
  database, and a new feature folder in the UI.
- **Passphrase unlock.** A passphrase-based `KeySource` (Argon2id, wrapped key) for
  systems without a keychain.

## What's inside

- `apps/desktop` — Tauri 2 desktop app (React 19, Vite 8, Tailwind 4, Rust)
- Shared tooling: oxlint, oxfmt, Knip, TypeScript 7, Turbo
- `@repo/logger` — structured JSON logging for Cloud Run / Cloud Logging
- pnpm catalog for versions, with supply-chain guards: a 14-day release-age wait,
  a trust-downgrade check, and no git/tarball subdependencies
- Claude Code settings in `.claude/`

```text
apps/desktop           Tauri 2 app: React frontend in src/, Rust in src-tauri/
packages/logger        pino logger, Cloud Logging shaped JSON
tooling/typescript     shared tsconfig presets (base, node, react)
tooling/github         composite action: pnpm + Node + install; secrets-scan script
.oxlintrc.json         single root lint config for the whole repo
.oxfmtrc.jsonc         single root format config for the whole repo
```

## Getting started

Requires Node.js 24.22.1 (`.nvmrc`) and pnpm 12.6.0 (`packageManager` in `package.json`).

```bash
pnpm install
```

The desktop app also needs Rust via rustup; `rust-toolchain.toml` pins the version.
On Windows install the MSVC Build Tools and WebView2, plus the VBSCRIPT optional
feature to build the MSI installer. On Linux install webkit2gtk 4.1. Under WSL2 the
app runs through WSLg once webkit2gtk, the SQLCipher build tools below and a Secret
Service (such as GNOME Keyring) are installed, and Ollama is reachable on WSL's own
loopback (mirrored networking, or Ollama installed inside WSL). Otherwise use a Windows
checkout.

The chat database is SQLCipher with a vendored OpenSSL, compiled from source on the
first build. That needs `perl`, `make` and a C compiler: on Windows, Strawberry Perl
plus the MSVC Build Tools; on Linux, `perl` and `build-essential` (or your distro's
equivalent).

```bash
pnpm --filter desktop dev        # open the app window (Vite on port 1420)
pnpm --filter desktop build:app  # release build and installers
```

Start Ollama first (`ollama serve`, or the Ollama app) and pull a model, for example
`ollama pull llama3.2`. The app expects it at `http://127.0.0.1:11434`; Settings can
change the port or pick another loopback address.

### Keyboard

| Key         | In the chat                    |
| ----------- | ------------------------------ |
| Enter       | Send the message               |
| Shift+Enter | New line                       |
| Esc         | Stop the reply being generated |

Esc works wherever focus is in the chat, except in the sidebar's rename field and in a
message being edited, where it cancels the rename or the edit. In the message editor,
Enter saves and resends.

### Building an installer

Installers are built locally; there is no CI build and no release workflow.

```bash
pnpm --filter desktop build:app
```

On Windows this writes an MSI and an NSIS setup program under
`apps/desktop/src-tauri/target/release/bundle/` (`msi/` and `nsis/`). The installers
are not code-signed, so Windows SmartScreen warns before the first run.

From WSL, one command builds the Windows installer without a separate Windows checkout:

```bash
pnpm --filter desktop build:windows
```

It exports the committed `HEAD` (uncommitted changes are left out) to
`%LOCALAPPDATA%\pcai-build`, then runs the build there through Windows PowerShell:
a fresh `src\`, `pnpm install --frozen-lockfile` and `build:app --bundles nsis`, with
`target\` kept between runs so SQLCipher compiles only once. It prints the path of the
setup program it copies to `out\`. Windows needs Node, pnpm (`corepack enable`),
rustup, Strawberry Perl and the MSVC Build Tools; the script names any that are
missing, with the `winget` commands to install them. Only the NSIS installer is
built, so the VBSCRIPT feature is not needed.

Only a bundled app enforces the production CSP, and release builds have no devtools.
To check for CSP violations, build a debug bundle with
`pnpm --filter desktop build:app --debug` (written under `target/debug/bundle/`), run
it, and open its devtools console (right-click → Inspect).

## Where is my data

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

## Commands

```bash
pnpm lint           # type-aware linting, whole repo in one oxlint process
pnpm knip           # unused files, exports and dependencies
pnpm typecheck      # TypeScript 7 (tsc) across the repo
pnpm test           # test suite
pnpm build          # production builds (frontend only, no Rust)
pnpm format         # oxfmt write
pnpm format:check   # oxfmt check (CI gate)
pnpm secrets:scan   # gitleaks secret scan over git history

pnpm --filter desktop rust:fmt    # cargo fmt --check
pnpm --filter desktop rust:check  # cargo clippy, warnings are errors
pnpm --filter desktop rust:test   # cargo test
```

CI does not build or check Rust. Run the three `rust:*` scripts, the dev window and
`build:app` locally before merging a desktop change; the installer check needs Windows.

oxlint prints nothing when there are no findings, so silent output means clean.

CI runs typecheck, lint, knip, format-check, test, build and a gitleaks secret scan on
push to `main` and on PRs, plus CodeQL code scanning. `secrets:scan` needs either
`gitleaks` v8.19+ on PATH or a running Docker daemon.

## Repository settings

- CodeQL (the `codeql` CI job) is free on public repos; private repos need GitHub Code
  Security. If "default setup" is enabled under Settings → Code security, turn it off —
  GitHub rejects the workflow's uploads while it is on.
- Under Settings → Code security, turn on **Dependabot alerts** and leave **Dependabot
  security updates** off: you get notified about vulnerable dependencies without
  automatic PRs. There is deliberately no `.github/dependabot.yml`, so no version-update
  PRs either.
