import type { LogFields } from "./types";

/**
 * pino looks up its per-key serialiser and stringifier with plain bracket
 * access on ordinary objects (`serializers[key]`, `stringifiers[key]`), so a
 * top-level field named after an `Object.prototype` member resolves to an
 * inherited function instead of `undefined`. The results are not subtle:
 * `hasOwnProperty` throws `TypeError: Cannot convert undefined or null to
 * object` out of the log call, and `toString` emits
 * `"toString":[object Undefined]` — an unparseable line, which Cloud Logging
 * drops.
 *
 * That turns `logger.info("request", req.query)` into a remote crash or a way
 * to make specific lines disappear, so these keys are removed before anything
 * reaches pino. Only top-level keys are affected; nested objects go through
 * `JSON.stringify` and are already safe.
 */
const PROTOTYPE_KEYS: ReadonlySet<string> = new Set(
  Object.getOwnPropertyNames(Object.prototype)
);

/**
 * Keys Cloud Logging reads by name. Bindings are written ahead of call-site
 * fields and nothing de-duplicates them, so a call-site field of the same name
 * is emitted second — and every JSON parser keeps the last value. That lets a
 * caller-supplied object silently rewrite the line's own metadata: a
 * `{"severity":"DEBUG"}` field on a `logger.error(...)` call files the entry as
 * DEBUG, where no `severity >= ERROR` alert will ever see it, and a forged
 * `time` or trace id backdates the entry or attaches it to someone else's
 * request.
 *
 * Bindings may still set these — `child(parseTraceHeaders(...))` is the
 * supported way to attach trace fields, and the app owns its own bindings.
 * Only call-site fields are restricted.
 */
const RESERVED_KEYS: ReadonlySet<string> = new Set([
  // Read by name and promoted out of the payload.
  "severity",
  "message",
  "httpRequest",
  "stack_trace",
  // Every timestamp form Cloud Logging accepts, not just the one we emit —
  // forging any of them backdates the entry out of an incident window.
  "time",
  "timestamp",
  "timestampSeconds",
  "timestampNanos",
  // `insertId` is the deduplication key: a forged repeat can suppress a later
  // genuine entry. `labels` and `operation` drive filtering and request
  // grouping, `sourceLocation` the reported call site.
  "logging.googleapis.com/insertId",
  "logging.googleapis.com/labels",
  "logging.googleapis.com/operation",
  "logging.googleapis.com/sourceLocation",
  "logging.googleapis.com/spanId",
  "logging.googleapis.com/trace",
  "logging.googleapis.com/trace_sampled",
]);

const copyWithout = (
  fields: LogFields,
  drop: (key: string) => boolean
): LogFields => {
  let dropped = false;
  const safe: Record<string, unknown> = {};

  for (const key of Object.keys(fields)) {
    if (drop(key)) {
      dropped = true;
      continue;
    }
    safe[key] = fields[key];
  }

  // Returning the original when nothing changed keeps the common path
  // allocation-free; pino never mutates what it is handed.
  return dropped ? safe : fields;
};

/** Call-site fields: drop both unusable and reserved keys. */
const sanitizeFields = (fields: LogFields): LogFields =>
  copyWithout(
    fields,
    (key) => PROTOTYPE_KEYS.has(key) || RESERVED_KEYS.has(key)
  );

/** Bindings: drop only the keys pino cannot represent. */
const sanitizeBindings = (bindings: LogFields): LogFields =>
  copyWithout(bindings, (key) => PROTOTYPE_KEYS.has(key));

export { sanitizeBindings, sanitizeFields };
