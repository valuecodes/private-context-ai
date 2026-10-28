# AGENTS.md

Guidelines for AI agents and contributors working in this Turborepo monorepo.

`CLAUDE.md` is a symlink to this file. Never edit `CLAUDE.md` directly.

---

## Structure

### Apps (`apps/`)

| Name    | Filter    | Description                                                     |
| ------- | --------- | --------------------------------------------------------------- |
| desktop | `desktop` | Tauri 2 desktop app: React 19 + Vite 8 + Tailwind 4, Rust shell |

`apps/desktop/src/lib/ipc.ts` is the only module that imports `@tauri-apps/api`;
everything else calls its typed wrappers, so `vi.mock("~/lib/ipc")` is the one test
seam. The Rust side lives in `apps/desktop/src-tauri`.

### Packages (`packages/`)

| Name       | Filter             | Description                                                      |
| ---------- | ------------------ | ---------------------------------------------------------------- |
| logger     | `@repo/logger`     | pino logger emitting Cloud Logging shaped JSON                   |
| typescript | `@repo/typescript` | Shared tsconfig presets (`base.json`, `node.json`, `react.json`) |

CI helpers are not workspaces: the composite setup action lives in
`.github/actions/setup/` and the gitleaks script in `.github/scripts/`.

Apps may import packages; packages must never import apps. oxlint bans `../`
imports and deep `@repo/*/src` imports: import other workspaces by package name,
and inside an app use the `~/` alias (`src/*`) to go up the tree.

---

## Commands

**Prerequisites:** Node.js 24 (exact version in `.nvmrc`), pnpm 12.11.2 (`packageManager` in root `package.json`).

```bash
pnpm install                     # Install all dependencies
pnpm --filter desktop dev        # Tauri dev window (Vite on port 1420)
pnpm --filter desktop build:app  # Rust release build + installers
pnpm --filter desktop build:windows # From WSL: Windows installer of HEAD, built on Windows
pnpm --filter desktop rust:fmt   # cargo fmt --check
pnpm --filter desktop rust:check # cargo clippy --all-targets --locked -D warnings
pnpm --filter desktop rust:test  # cargo test --locked

pnpm lint                        # oxlint, one process over the whole repo
pnpm knip                        # unused files, exports and dependencies
pnpm typecheck                   # turbo run typecheck
pnpm test                        # turbo run test
pnpm build                       # turbo run build
pnpm format                      # oxfmt, writes in place
pnpm format:check                # oxfmt --check (no writes)
pnpm secrets:scan                # gitleaks over the full git history
pnpm clean                       # turbo run clean
```

`lint` and `knip` do not go through Turbo — each is a single process over the whole
repo. oxlint is configured by the root `.oxlintrc.json` (including `import/no-cycle`)
and prints nothing when there are no findings, so silent output means clean. Knip is
configured by the root `knip.jsonc` and exits 0 when clean. Formatting is oxfmt
(Prettier-compatible), configured by the root `.oxfmtrc.jsonc`, which also sorts
imports and Tailwind classes.

Nothing formats on edit, and the pre-commit hook only checks formatting: run
`pnpm format` yourself before committing.

Git hooks live in `.githooks/` and are plain shell scripts, with no hook manager.
`pnpm install` enables them through the root `prepare` script. The setting goes in
the clone's shared git config, so it covers every worktree, and a branch without
`.githooks/` simply runs no hooks. If you cloned before the hooks existed and have
not run `pnpm install` since, run `git config core.hooksPath .githooks` once.
`pre-commit` runs `format:check`, `lint` and `secrets:scan --staged`. The secrets
scan fails closed, so the commit is refused when neither gitleaks nor Docker is
available. `pre-push` runs `typecheck`, `test` and `knip`. CI still runs every
check, and neither hook runs the `rust:*` scripts. `format:check` and `lint` read
the working tree, not the staged snapshot, so stage everything you mean to commit.

`secrets:scan` runs gitleaks (`.github/scripts/secrets-scan.sh`, version pinned
there) using a local `gitleaks` v8.19+ if one is on PATH, otherwise the pinned Docker image.
It exits 0 when clean and 1 when it finds a leak.

CI checks are defined once in the reusable `.github/workflows/checks.yml`: typecheck,
lint, knip, format:check, test, build, secrets-scan, CodeQL code scanning
(`javascript-typescript`) and workflow-lint. `feature.yml` calls it on PRs and
`main.yml` on push to `main`. workflow-lint calls the reusable workflow from
`valuecodes/automation`: actionlint (with shellcheck) and zizmor over the workflow and
action files. Each tool reads `.github/actionlint.yaml` / `zizmor.yml` if added.
`all-passed` in `feature.yml` fans the checks in and is the one check to require in
branch protection. The typecheck, test and build jobs restore Turbo's `.turbo` cache
from the previous run of the same task, so unchanged packages replay instead of
rerunning. CI covers the frontend only; see Rust below.

Dependabot (`.github/dependabot.yml`) bumps the SHA-pinned actions and reusable workflows
monthly in one grouped PR; npm and Cargo dependencies are updated by hand.

`maintenance-dashboard.yml` calls the maintenance dashboard workflow from
`valuecodes/automation` daily.
It keeps one issue, **Maintenance dashboard** (label `maintenance-dashboard`), listing
what is due: `catalog:` updates, pnpm (`packageManager`), gitleaks (`GITLEAKS_VERSION`
in the secrets-scan script), Node.js (`.nvmrc`) and `pnpm audit` findings. Like
`minimumReleaseAge`, a release shows up only once it is 14 days old; a Node.js security
release shows up right away and is flagged. It never opens PRs or commits; doing the
updates stays manual. For a Node.js bump, change only `.nvmrc` (the one place that
holds the exact version). If `GITLEAKS_VERSION` moves out of
`.github/scripts/secrets-scan.sh`, pass the new path as the `gitleaks-script` input.
The dashboard and workflow-lint callers pin the same `valuecodes/automation` commit,
so bump them together.

### Rust (`apps/desktop/src-tauri`)

`pnpm build` is frontend only (`vite build`); it never compiles Rust, and CI does not
build or check Rust at all. Crates are pinned exactly (`=x.y.z`) and `Cargo.lock` is
committed. Before a PR that touches the desktop app merges, run on a Windows checkout:
the three `rust:*` scripts above, `pnpm --filter desktop dev` (window opens, `invoke`
works), and `pnpm --filter desktop build:app`. For the CSP check, also build
`pnpm --filter desktop build:app --debug`, open that bundle and check its devtools
console for CSP violations: release builds have no devtools (the `devtools` Cargo
feature is off).

On desktop, `tauri dev` loads the Vite server directly, so Tauri applies neither `csp`
nor `devCsp` there (Tauri proxies the dev server, and so applies `devCsp`, only on
mobile). The production CSP is enforced only in a bundled app, release or debug, which
is why the CSP check runs against the `build:app --debug` output.

Prerequisites: rustup. The toolchain is pinned by `src-tauri/rust-toolchain.toml`,
which rustup only honours when cargo runs inside `src-tauri`, so the `rust:*` scripts
`cd` there. Windows: MSVC Build Tools, WebView2, and the VBSCRIPT optional feature for
the MSI installer. Linux: webkit2gtk 4.1, and later `perl`, `make` and a C compiler for
SQLCipher.

WSL2 runs `tauri dev` (through WSLg), clippy and the tests once webkit2gtk 4.1, the
SQLCipher build tools and a Secret Service (such as GNOME Keyring) are installed, with
Ollama on WSL's own loopback. A WSL2 setup without them can only run `rust:fmt`; say
which checks ran in the PR description. `build:app` for the Windows installer always
runs on Windows; from WSL, `build:windows` runs it there on an export of HEAD.

### Security invariants

`docs/security.md` explains these; a change that breaks one needs
discussion first.

- **No egress except Ollama on loopback.** The webview has no network (strict CSP, no
  Tauri plugins). Only Rust talks to Ollama, through `ollama/client.rs`, which refuses
  non-loopback hosts, proxies and redirects; `reqwest` has no TLS feature.
- **The model gets a fixed, read-only tool set, never with `format`, only if its
  audited `/api/show` lists `tools`.** The display-only `render_chart`
  (`ollama/chart_tool.rs`) only when the user turns Charts on, on a turn's first request,
  and the request after a chart has no `tools`. The read-only `list_files` /
  `search_files` / `read_file` (`ollama/file_tools.rs`) only with Files on in a project
  that has files: no tool takes a disk path, writes or reaches the network; at most 4
  requests per turn and 4 calls per request, results capped (16 KiB each, 64 KiB per
  turn), escaped as data and never saved or replayed, and the last request has no
  `tools`. Rust validates every call and runs it itself.
  Charts are drawn as SVG by local React code, with model text only as text nodes;
  `think` is sent only as a boolean or an advertised level, only to models whose
  audited `/api/show` lists `thinking`, and is `false` unless the user turns it on.
  Thinking text is stored encrypted, shown collapsed, never sent back to the model.
  Replies are rendered as Markdown into React elements with raw HTML disabled
  (`skipHtml`, no `rehype-raw`, no `dangerouslySetInnerHTML`); links and images are
  inert (no `href`, nothing fetched) and code highlighting is local JS.
- **Encrypted at rest.** All saved chat data lives in the SQLCipher database, only Rust
  opens it, and its key never reaches the webview. Temporary chats live only in Rust
  process memory (`temporary.rs`) and are never written to disk; their audit rows are.
- **Observable.** Every outbound request writes an audit row before its socket opens,
  including project memory summaries (`ollama.memory`). Logs hold metadata only;
  prompt, reply and memory text stay in `Redacted`.
- **One IPC seam.** Only `apps/desktop/src/lib/ipc.ts` imports `@tauri-apps/api`.

---

## Rules

- Keep diffs tight and focused; no drive-by refactors or new tooling without discussion.
- Never commit secrets, credentials, or `.env` files. All code must be public-safe.
- Never bypass the git hooks with `--no-verify`; fix the failure instead.
- TypeScript only: no `.js`, `.jsx`, `.cjs` or `.mjs` files, config files included
  (`*.config.ts`). A browser script that must stay a classic script, such as
  `apps/desktop/src/theme-init.ts`, is compiled by a Vite plugin, not committed as JS.
- Add dependencies to the correct workspace with `pnpm --filter <package> add <dep>`.
  Every external version lives in the `catalog:` block of `pnpm-workspace.yaml`
  and manifests reference it as `catalog:`; `@repo/*` deps stay `workspace:*`.
  Update with `pnpm up --latest -r`; `updateConfig.ignoreDependencies` holds
  `@types/node` to the `.nvmrc` major and `@tauri-apps/api` / `@tauri-apps/cli` to
  the `tauri` crate in `Cargo.toml`, so bump those by hand.
- Every install enforces the supply-chain settings in `pnpm-workspace.yaml`
  (`minimumReleaseAge`, `trustPolicy: no-downgrade`) plus pnpm 12's default
  `blockExoticSubdeps`. When one fails, investigate: never disable it, and never
  exclude a whole package to get past it.
