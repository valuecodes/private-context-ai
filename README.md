# Private Context AI

A secure local AI sandbox, in development. The planned MVP is a Tauri 2 desktop app that
chats with a user-installed Ollama server on loopback. History is encrypted at rest, the
model gets no tools, and the app contacts nothing but Ollama. Today `apps/desktop` is the
hardened app shell: a strict Content Security Policy, narrowed Tauri permissions and no
Ollama calls yet.

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
app cannot run; use a Windows checkout.

```bash
pnpm --filter desktop dev        # open the app window (Vite on port 1420)
pnpm --filter desktop build:app  # release build and installers
```

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
`build:app` on Windows before merging a desktop change.

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
