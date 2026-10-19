import pino from "pino";

/**
 * Cloud Error Reporting evaluates `stack_trace`, then `exception`, then
 * `message`, and only falls back to scanning every field when none of the three
 * is present. `message` here is always a static human string, so that fallback
 * never runs: a top-level `stack_trace` is the only thing that turns an ERROR
 * line into a grouped error event.
 *
 * `@type` and `serviceContext` are deliberately absent. Setting `@type` would
 * promote *every* ERROR line into Error Reporting, including the ones that
 * describe a policy outcome rather than an exception. The rule this encodes is
 * narrower and more useful: a line carrying a real `Error` becomes an error
 * event, and nothing else does.
 */
const withStackTrace = (
  object: Record<string, unknown>
): Record<string, unknown> => {
  const error: unknown = object.err;
  if (!(error instanceof Error)) {
    return object;
  }

  // pino runs formatters.log BEFORE the per-key serialisers, so `err` is still
  // a live Error here. Going through the std serialiser rather than reading
  // `error.stack` directly folds a `cause` chain into the frames, which is what
  // makes Error Reporting group on the root cause instead of the wrapper.
  return { ...object, stack_trace: pino.stdSerializers.err(error).stack };
};

export { withStackTrace };
