# Playground

Vite + React playground for experimenting with UI and agent workflows in this monorepo.

## Stack

- Vite 8 + React 19
- Tailwind CSS 4
- Vitest for unit tests

## Getting started

From the repo root:

```bash
pnpm install
pnpm --filter playground dev
```

Open http://localhost:3000 to view the app.

## Common commands

| Task      | Command                              |
| --------- | ------------------------------------ |
| Dev       | `pnpm --filter playground dev`       |
| Build     | `pnpm --filter playground build`     |
| Preview   | `pnpm --filter playground preview`   |
| Typecheck | `pnpm --filter playground typecheck` |
| Test      | `pnpm --filter playground test`      |
| Clean     | `pnpm --filter playground clean`     |
| Deploy    | `pnpm --filter playground deploy`    |

Linting and formatting are repo-wide: run `pnpm lint` / `pnpm format` from the root.

## Styling

`src/globals.css` imports Tailwind and sets `@source` scanning. There is no shared
theme package, so use plain Tailwind utilities.

## Deploy (Cloudflare)

The app deploys as static assets on a Cloudflare Worker. `wrangler.jsonc` serves
`dist/`, and `public/_headers` (copied into `dist/` by Vite) sets the security and
cache headers. `wrangler` is a pinned devDependency, so the deploy uses the lockfile
version rather than `npx`.

There is no CI deploy job. To connect the repo in the Cloudflare dashboard
(Workers & Pages → Create → Import a repository):

| Setting        | Value                       |
| -------------- | --------------------------- |
| Root directory | `apps/playground`           |
| Build command  | `pnpm build`                |
| Deploy command | `pnpm exec wrangler deploy` |

Also set these build variables (Settings → Build → Variables), so Cloudflare installs
dependencies with the repo's toolchain. The install runs before the build command,
so pinning versions there would be too late:

| Variable       | Value     |
| -------------- | --------- |
| `NODE_VERSION` | `24.21.0` |
| `PNPM_VERSION` | `12.6.0`  |

Keep them in step with `.nvmrc` and the root `packageManager` field.

### Web Analytics

Cloudflare Web Analytics is optional. Add the site in the dashboard (Web
Analytics → Add a site) with manual JS snippet installation, not automatic
injection, which would load a second beacon. Copy the `token` from its snippet
into the build variable `VITE_CF_BEACON_TOKEN` (Settings → Build → Variables).
Set it on the production build only: preview builds then have no beacon and
don't report into the site. The token is public: the build puts the beacon
`<script>` into `index.html`. Without the variable the page has no beacon.

To deploy from your machine instead, run `pnpm --filter playground exec wrangler login` once, then
`pnpm --filter playground deploy`.

The Content-Security-Policy in `public/_headers` allows only this origin plus
Cloudflare Web Analytics. Loading scripts, fonts, images or APIs from any other
origin needs a matching entry there, or the browser blocks it in production (the
Vite dev server does not apply `_headers`).
