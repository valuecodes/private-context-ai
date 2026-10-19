import type { ReactNode } from "react";

type SectionProps = {
  readonly id: string;
  readonly title: string;
  readonly description?: string;
  readonly children: ReactNode;
};

const Section = ({ id, title, description, children }: SectionProps) => (
  <section aria-labelledby={id} className="flex flex-col gap-5">
    <div className="flex flex-col gap-1">
      <h2
        id={id}
        className="text-lg font-semibold tracking-tight text-zinc-900 dark:text-zinc-50"
      >
        {title}
      </h2>
      {description === undefined ? null : (
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          {description}
        </p>
      )}
    </div>
    {children}
  </section>
);

export { Section };
