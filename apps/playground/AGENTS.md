# AGENTS.md - apps/playground

This directory inherits `/AGENTS.md`. This file lists only additions and overrides specific to the playground app.

---

## What This Workspace Is

- Vite 8 + React 19 app for fast experiments.
- Tailwind CSS 4.
- Single-page app rendered from `src/home.tsx`.

---

## Workspace Commands

| Task      | Command                              |
| --------- | ------------------------------------ |
| Dev       | `pnpm --filter playground dev`       |
| Build     | `pnpm --filter playground build`     |
| Preview   | `pnpm --filter playground preview`   |
| Typecheck | `pnpm --filter playground typecheck` |
| Test      | `pnpm --filter playground test`      |
| Clean     | `pnpm --filter playground clean`     |
| Deploy    | `pnpm --filter playground deploy`    |

Or run repo-wide via root `pnpm typecheck` / `pnpm test` / `pnpm build`.

There are no per-workspace `lint` or `format` scripts: root `pnpm lint` (oxlint) and
`pnpm format` (oxfmt) each cover the whole repo.

---

## Local Conventions (Deltas from Root)

### Entry Point

- `src/main.tsx` mounts the app and renders `Home` from `src/home.tsx`.

### Styling

- `src/globals.css` imports Tailwind and declares its `@source` scanning.
- PostCSS config is `.postcssrc.json`. Keep that name: Vite does not look for
  `postcss.config.json`, and a missing config makes Tailwind emit no utilities.
- Plain Tailwind utilities — there is no shared theme package, so semantic
  tokens like `text-muted-foreground` are not available.

### TypeScript

- `tsconfig.json` sets `"types": ["vite/client"]`. This is load-bearing:
  without it, TypeScript 7 rejects `import "./globals.css"` with TS2882.

### Vite Notes

- `~/*` maps to `src/*` (tsconfig `paths`, read by Vite and Vitest through
  `resolve.tsconfigPaths`). Use it instead of `../`, which oxlint bans.

### Deploy

- Cloudflare static assets: `wrangler.jsonc` serves `dist/`; `public/_headers`
  sets the CSP and cache headers. Deploys are connected in the Cloudflare
  dashboard, not CI (settings in `README.md`).
- `VITE_CF_BEACON_TOKEN` is an optional build variable, the Web Analytics token:
  `vite.config.ts` adds the beacon script to `index.html` only when it is set
  (production builds only, and the dashboard site on manual installation, so
  there is one beacon).
- `compatibility_date` cannot be newer than the pinned workerd's date
  (`1.YYYYMMDD.x`); bump the two together.

---

## Footguns / Gotchas

1. **`build` does not typecheck** - Vite strips types without checking them. Run
   `pnpm --filter playground typecheck` (CI runs it as its own job).
2. **Tailwind scanning** - `@source` in `src/globals.css` covers `src/` only; add an entry
   for any class-bearing files outside it.
3. **CSP is production-only** - `public/_headers` allows only `'self'` plus
   Cloudflare Web Analytics. Anything from another origin works under `vite dev`
   and breaks once deployed, until `_headers` allows it.
