import { common, createLowlight } from "lowlight";
import { Check, Copy } from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";

import { copyText } from "~/lib/clipboard";

const lowlight = createLowlight(common);

/** Longer code is shown plain: highlighting runs on the UI thread. */
const HIGHLIGHT_MAX_CHARS = 64 * 1024;

const COPIED_MS = 1500;

/** The parts of a hast node that `renderHast` reads. */
type HastNode = {
  type: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
};

const classNameOf = (
  properties: HastNode["properties"]
): string | undefined => {
  const className = properties?.className;
  return Array.isArray(className) ? className.join(" ") : undefined;
};

/**
 * Renders a highlight tree as nested `<span>`s with their classes only. Every
 * other property and the tag name are dropped, so nothing in the tree can add
 * an attribute or an element.
 */
const renderHast = (node: HastNode, key?: number): ReactNode => {
  const children = node.children?.map((child, index) =>
    renderHast(child, index)
  );
  if (node.type === "root") {
    return children;
  }
  if (node.type === "text") {
    return node.value;
  }
  if (node.type === "element") {
    return (
      <span key={key} className={classNameOf(node.properties)}>
        {children}
      </span>
    );
  }
  return null;
};

const highlighted = (
  language: string | null,
  text: string,
  highlight: boolean
): ReactNode => {
  if (
    !highlight ||
    language === null ||
    text.length > HIGHLIGHT_MAX_CHARS ||
    !lowlight.registered(language)
  ) {
    return text;
  }
  return renderHast(lowlight.highlight(language, text));
};

type CodeBlockProps = {
  language: string | null;
  text: string;
  /** False while the reply streams, so a growing block is not re-highlighted. */
  highlight: boolean;
};

const CodeBlock = ({ language, text, highlight }: CodeBlockProps) => {
  const [copied, setCopied] = useState(false);

  const copy = () => {
    void copyText(text).then((ok) => {
      if (ok) {
        setCopied(true);
        setTimeout(() => {
          setCopied(false);
        }, COPIED_MS);
      }
    });
  };

  return (
    <div className="my-3 overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-800">
      <div className="flex items-center justify-between bg-zinc-100 px-3 py-1 text-xs text-zinc-500 dark:bg-zinc-900 dark:text-zinc-400">
        <span>{language ?? "text"}</span>
        <button
          type="button"
          onClick={copy}
          aria-label={copied ? "Copied" : "Copy code"}
          title="Copy code"
          className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-zinc-200 dark:hover:bg-zinc-800"
        >
          {copied ? (
            <Check aria-hidden="true" className="size-3.5" />
          ) : (
            <Copy aria-hidden="true" className="size-3.5" />
          )}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="overflow-x-auto bg-white p-3 text-[13px] leading-relaxed dark:bg-zinc-950">
        <code className="hljs">{highlighted(language, text, highlight)}</code>
      </pre>
    </div>
  );
};

export { CodeBlock, HIGHLIGHT_MAX_CHARS, renderHast };
