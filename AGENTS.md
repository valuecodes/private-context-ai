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

| Name   | Filter         | Description                                    |
| ------ | -------------- | ---------------------------------------------- |
| logger | `@repo/logger` | pino logger emitting Cloud Logging shaped JSON |

### Tooling (`tooling/`)

| Name       | Filter             | Description                                                      |
| ---------- | ------------------ | ---------------------------------------------------------------- |
| typescript | `@repo/typescript` | Shared tsconfig presets (`base.json`, `node.json`, `react.json`) |
| github     | `@repo/github`     | GitHub Actions composite setup action, gitleaks `secrets-scan`   |

Apps may import packages; packages must never import apps. oxlint bans `../`
imports and deep `@repo/*/src` imports: import other workspaces by package name,
and inside an app use the `~/` alias (`src/*`) to go up the tree.

---

## Commands

**Prerequisites:** Node.js 24.21.0 (`.nvmrc`), pnpm 12.6.0 (`packageManager` in root `package.json`).

```bash
pnpm install                     # Install all dependencies
pnpm --filter desktop dev        # Tauri dev window (Vite on port 1420)
pnpm --filter desktop build:app  # Rust release build + installers
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

There is no post-edit formatting hook: run `pnpm format` yourself before committing.

`secrets:scan` runs gitleaks (`tooling/github/scripts/secrets-scan.sh`, version pinned
there) using a local `gitleaks` v8.19+ if one is on PATH, otherwise the pinned Docker image.
It exits 0 when clean and 1 when it finds a leak.

CI (`.github/workflows/`) runs typecheck, lint, knip, format-check, test, build,
secrets-scan and CodeQL code scanning (`javascript-typescript` and `actions`) on push
to `main` and on PRs.

### Rust (`apps/desktop/src-tauri`)

`pnpm build` is frontend only (`vite build`); it never compiles Rust, and CI does not
build or check Rust at all. Crates are pinned exactly (`=x.y.z`) and `Cargo.lock` is
committed. Before a PR that touches the desktop app merges, run on a Windows checkout:
the three `rust:*` scripts above, `pnpm --filter desktop dev` (window opens, `invoke`
works), and `pnpm --filter desktop build:app`, then open the built app and check its
devtools console for CSP violations.

On desktop, `tauri dev` loads the Vite server directly, so Tauri applies neither `csp`
nor `devCsp` there (Tauri proxies the dev server, and so applies `devCsp`, only on
mobile). The production CSP is enforced only in the bundled app, which is why the CSP
check runs against the `build:app` output.

Prerequisites: rustup. The toolchain is pinned by `src-tauri/rust-toolchain.toml`,
which rustup only honours when cargo runs inside `src-tauri`, so the `rust:*` scripts
`cd` there. Windows: MSVC Build Tools, WebView2, and the VBSCRIPT optional feature for
the MSI installer. Linux: webkit2gtk 4.1, and later `perl`, `make` and a C compiler for
SQLCipher.

WSL2 lacks the webkit2gtk libraries and an OS keychain, so `tauri dev`, clippy and the
tests do not run there. From WSL2 only `rust:fmt` works; say so in the PR description
and run the rest from a Windows checkout.

---

## Rules

- Keep diffs tight and focused; no drive-by refactors or new tooling without discussion.
- Never commit secrets, credentials, or `.env` files. All code must be public-safe.
- Add dependencies to the correct workspace with `pnpm --filter <package> add <dep>`.
  Versions shared by more than one package go in the `catalog:` block of
  `pnpm-workspace.yaml`; single-consumer deps are pinned inline.
- Every install enforces the supply-chain settings in `pnpm-workspace.yaml`
  (`minimumReleaseAge`, `trustPolicy: no-downgrade`) plus pnpm 12's default
  `blockExoticSubdeps`. When one fails, investigate: never disable it, and never
  exclude a whole package to get past it.
