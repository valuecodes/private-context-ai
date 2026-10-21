import { Link } from "lucide-react";
import { createContext, useContext } from "react";
import type { ComponentProps } from "react";
import ReactMarkdown from "react-markdown";
import type { Components, ExtraProps } from "react-markdown";
import remarkGfm from "remark-gfm";

import { CodeBlock } from "~/features/chat/code-block";
import { copyText } from "~/lib/clipboard";

const HighlightContext = createContext(true);

type HastChild = NonNullable<ExtraProps["node"]>["children"][number];

const textOf = (node: HastChild): string => {
  if (node.type === "text") {
    return node.value;
  }
  return node.type === "element" ? node.children.map(textOf).join("") : "";
};

const LANGUAGE_PREFIX = "language-";

const languageOf = (className: unknown): string | null => {
  const found = Array.isArray(className)
    ? className.find(
        (name): name is string =>
          typeof name === "string" && name.startsWith(LANGUAGE_PREFIX)
      )
    : undefined;
  return found === undefined ? null : found.slice(LANGUAGE_PREFIX.length);
};

/** Block code: `pre > code` becomes a `CodeBlock`; inline code stays as is. */
const Pre = ({ node }: ComponentProps<"pre"> & ExtraProps) => {
  const highlight = useContext(HighlightContext);
  const code = node?.children[0];
  if (code?.type !== "element" || code.tagName !== "code") {
    return null;
  }
  return (
    <CodeBlock
      language={languageOf(code.properties.className)}
      text={textOf(code).replace(/\n$/, "")}
      highlight={highlight}
    />
  );
};

/**
 * A link is shown as text, never an `href`, so nothing in a reply can
 * navigate the webview. `href` is already blanked by react-markdown's URL
 * filter for `javascript:` and similar schemes.
 */
const InertLink = ({ href, children }: ComponentProps<"a"> & ExtraProps) => {
  if (href === undefined || href === "") {
    return <span>{children}</span>;
  }
  return (
    <span className="markdown-link" title={href}>
      {children}
      <button
        type="button"
        onClick={() => {
          void copyText(href);
        }}
        aria-label="Copy link"
        title="Copy link"
        className="ml-0.5 inline-flex rounded p-0.5 align-middle text-zinc-500 hover:bg-zinc-200 dark:text-zinc-400 dark:hover:bg-zinc-800"
      >
        <Link aria-hidden="true" className="size-3" />
      </button>
    </span>
  );
};

/** An image is shown as its alt text: nothing is ever fetched. */
const AltText = ({ alt }: ComponentProps<"img"> & ExtraProps) => (
  <span>{alt}</span>
);

const COMPONENTS: Components = { pre: Pre, a: InertLink, img: AltText };

const REMARK_PLUGINS = [remarkGfm];

type MarkdownProps = {
  text: string;
  /** Highlight code blocks; off while the reply streams. */
  highlight?: boolean;
};

/**
 * A reply rendered as Markdown into React elements. Raw HTML is skipped, so
 * the model can never inject markup.
 */
const Markdown = ({ text, highlight = true }: MarkdownProps) => (
  <HighlightContext value={highlight}>
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={REMARK_PLUGINS}
        components={COMPONENTS}
        skipHtml
      >
        {text}
      </ReactMarkdown>
    </div>
  </HighlightContext>
);

export { Markdown };
