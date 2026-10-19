import { CopyButton } from "~/components/copy-button";
import { Section } from "~/components/section";
import { commands, guardrails, layout, repoUrl, stack } from "~/content";

const Home = () => (
  <div className="min-h-screen bg-zinc-50 text-zinc-800 antialiased dark:bg-zinc-950 dark:text-zinc-300">
    <main className="mx-auto flex w-full max-w-4xl flex-col gap-16 px-5 py-16 sm:px-8 sm:py-24">
      <header className="flex flex-col gap-6">
        <p className="w-fit rounded-full border border-emerald-600/30 bg-emerald-50 px-3 py-1 text-xs font-medium text-emerald-700 dark:border-emerald-400/30 dark:bg-emerald-400/10 dark:text-emerald-300">
          Turborepo template
        </p>
        <h1 className="text-4xl font-semibold tracking-tight text-balance text-zinc-950 sm:text-5xl dark:text-white">
          Agentic Monorepo Starter
        </h1>
        <p className="max-w-2xl text-lg text-pretty text-zinc-600 dark:text-zinc-400">
          A strict, batteries-included monorepo for agent-assisted development:
          fast tooling, supply-chain guardrails and docs that coding agents
          actually read.
        </p>
        <div className="flex flex-wrap gap-3">
          <a
            href={repoUrl}
            className="rounded-lg bg-zinc-900 px-4 py-2 text-sm font-medium text-white transition hover:bg-zinc-700 focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:ring-offset-2 focus-visible:outline-none dark:bg-white dark:text-zinc-900 dark:hover:bg-zinc-200 dark:focus-visible:ring-offset-zinc-950"
          >
            View on GitHub
          </a>
          <a
            href="#quick-start"
            className="rounded-lg border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 transition hover:border-zinc-400 hover:text-zinc-950 focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:outline-none dark:border-zinc-700 dark:text-zinc-300 dark:hover:border-zinc-500 dark:hover:text-white"
          >
            Quick start
          </a>
        </div>
      </header>

      <Section
        id="stack"
        title="Stack"
        description="Current majors, pinned in the lockfile."
      >
        <ul className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
          {stack.map((item) => (
            <li
              key={item.name}
              className="flex flex-col gap-1 rounded-xl border border-zinc-200 bg-white p-4 dark:border-zinc-800 dark:bg-zinc-900"
            >
              <span className="flex items-baseline justify-between gap-2">
                <span className="font-medium text-zinc-900 dark:text-zinc-100">
                  {item.name}
                </span>
                <span className="font-mono text-xs text-emerald-700 dark:text-emerald-400">
                  {item.version}
                </span>
              </span>
              <span className="text-xs text-zinc-500">{item.note}</span>
            </li>
          ))}
        </ul>
      </Section>

      <Section
        id="quick-start"
        title="Quick start"
        description="Node 24 and pnpm 12, from the repo root."
      >
        <ul className="flex flex-col gap-3">
          {commands.map((item) => (
            <li
              key={item.label}
              className="flex flex-col gap-2 rounded-xl border border-zinc-200 bg-white p-4 dark:border-zinc-800 dark:bg-zinc-900"
            >
              <span className="text-xs font-medium tracking-wide text-zinc-500 uppercase">
                {item.label}
              </span>
              <div className="flex items-center justify-between gap-3">
                <code className="overflow-x-auto font-mono text-sm whitespace-nowrap text-zinc-900 dark:text-zinc-100">
                  <span className="text-emerald-600 select-none dark:text-emerald-400">
                    ${" "}
                  </span>
                  {item.command}
                </code>
                <CopyButton text={item.command} />
              </div>
            </li>
          ))}
        </ul>
      </Section>

      <Section
        id="guardrails"
        title="Guardrails"
        description="Enforced on every install and in CI, not left to review."
      >
        <ul className="grid gap-3 sm:grid-cols-2">
          {guardrails.map((item) => (
            <li
              key={item.title}
              className="flex flex-col gap-1.5 rounded-xl border border-zinc-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900"
            >
              <span className="font-medium text-zinc-900 dark:text-zinc-100">
                {item.title}
              </span>
              <span className="text-sm text-zinc-600 dark:text-zinc-400">
                {item.detail}
              </span>
            </li>
          ))}
        </ul>
      </Section>

      <Section
        id="layout"
        title="Layout"
        description="Apps may import packages; packages never import apps."
      >
        <pre className="overflow-x-auto rounded-xl border border-zinc-200 bg-white p-5 font-mono text-sm leading-relaxed text-zinc-700 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-300">
          {layout}
        </pre>
      </Section>

      <footer className="border-t border-zinc-200 pt-8 text-sm text-zinc-500 dark:border-zinc-800">
        This is the playground app. Edit{" "}
        <code className="rounded bg-zinc-200/70 px-1.5 py-0.5 font-mono text-xs text-zinc-800 dark:bg-zinc-800 dark:text-zinc-200">
          apps/playground/src/home.tsx
        </code>{" "}
        to start experimenting.
      </footer>
    </main>
  </div>
);

export { Home };
