# Development

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

## Building an installer

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
