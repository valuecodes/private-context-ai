const DOT = "size-2 rounded-full bg-zinc-400 motion-safe:animate-bounce";

/** Shown while the first token of a reply is on its way. */
const TypingIndicator = () => (
  <span className="inline-flex items-center gap-1 py-2">
    <span aria-hidden="true" className="inline-flex gap-1">
      <span className={`${DOT} [animation-delay:-0.3s]`} />
      <span className={`${DOT} [animation-delay:-0.15s]`} />
      <span className={DOT} />
    </span>
    <span className="sr-only">Assistant is typing</span>
  </span>
);

export { TypingIndicator };
