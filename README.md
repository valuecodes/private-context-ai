# Private Context AI

A secure local AI sandbox: a Tauri 2 desktop app that streams chat replies from an
Ollama server you run yourself, on this computer. Chat history is encrypted at rest, the
model gets no tools, the app contacts nothing but Ollama on loopback, and an Activity
view lists every request it made.

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
- **Rust ↔ Ollama.** Only Rust talks to Ollama: `/api/version`, `/api/tags` and a
  streamed `/api/chat`, over plain HTTP to a loopback address.
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
- **The model gets no tools.** `/api/chat` is sent without `tools`, `format` or
  `think`; the request type has no such fields. Replies are shown as plain text: never
  rendered as HTML, never executed.
- **History is encrypted at rest.** Everything is in a SQLCipher database whose random
  32-byte key lives in the OS keychain and never reaches the webview. Without a working
  keychain the app stores nothing.
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

Requires Node.js 24.21.0 (`.nvmrc`) and pnpm 12.6.0 (`packageManager` in `package.json`).

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

Esc works wherever focus is in the chat, except in the sidebar's rename field, where it
cancels the rename.

### Building an installer

Installers are built locally; there is no CI build and no release workflow.

```bash
pnpm --filter desktop build:app
```

On Windows this writes an MSI and an NSIS setup program under
`apps/desktop/src-tauri/target/release/bundle/` (`msi/` and `nsis/`). The installers
are not code-signed, so Windows SmartScreen warns before the first run. After
installing, open the app and check its devtools console for CSP violations: only the
bundled app enforces the production CSP.

## Where is my data

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
