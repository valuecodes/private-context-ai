type StackItem = {
  readonly name: string;
  readonly version: string;
  readonly note: string;
};

type Command = {
  readonly label: string;
  readonly command: string;
};

type Guardrail = {
  readonly title: string;
  readonly detail: string;
};

const repoUrl = "https://github.com/valuecodes/agentic-monorepo-starter";

const stack: readonly StackItem[] = [
  { name: "Turborepo", version: "2.11", note: "Cached task graph" },
  { name: "pnpm", version: "12", note: "Workspaces and catalogs" },
  { name: "TypeScript", version: "7", note: "Native Go compiler" },
  { name: "Vite", version: "8", note: "Rolldown-powered builds" },
  { name: "React", version: "19", note: "This page" },
  { name: "Tailwind CSS", version: "4", note: "Utility styling" },
  { name: "oxlint", version: "1.85", note: "Type-aware linting" },
  { name: "oxfmt", version: "0.71", note: "Prettier-compatible formatting" },
  { name: "Knip", version: "6", note: "Unused code and deps" },
  { name: "Vitest", version: "5", note: "Unit tests" },
];

const commands: readonly Command[] = [
  { label: "Install", command: "pnpm install" },
  { label: "Run this app", command: "pnpm --filter playground dev" },
  {
    label: "Check everything",
    command: "pnpm typecheck && pnpm lint && pnpm knip && pnpm test",
  },
];

const guardrails: readonly Guardrail[] = [
  {
    title: "Supply chain",
    detail:
      "Releases must be 14 days old, trust evidence can never downgrade, and exotic subdependencies are blocked.",
  },
  {
    title: "Secrets",
    detail:
      "gitleaks scans the full git history locally and in CI with a pinned version.",
  },
  {
    title: "Code scanning",
    detail: "CodeQL analyses the TypeScript and the GitHub Actions workflows.",
  },
  {
    title: "Boundaries",
    detail:
      "No ../ imports, no deep @repo/*/src imports, no import cycles, no barrel files.",
  },
  {
    title: "Dead code",
    detail: "Knip fails on unused files, exports and dependencies.",
  },
  {
    title: "Agent context",
    detail:
      "A root AGENTS.md, plus per-workspace additions where needed, tells coding agents the rules and footguns.",
  },
];

const layout = `apps/
  playground/        Vite + React app (this page)
packages/
  logger/            pino logger for Cloud Logging
tooling/
  typescript/        shared tsconfig presets
  github/            CI setup action, secrets scan`;

export { commands, guardrails, layout, repoUrl, stack };
