type Severity = "DEBUG" | "INFO" | "WARNING" | "ERROR" | "CRITICAL" | "DEFAULT";

// Google's LogSeverity names are not pino's level labels, and the two that
// differ are the two that matter: `warn` must become WARNING and `fatal` must
// become CRITICAL. Get either wrong and Cloud Logging files the entry under
// DEFAULT, where every `severity >= WARNING` filter and alert misses it.
const SEVERITY_BY_LABEL: Readonly<Partial<Record<string, Severity>>> = {
  trace: "DEBUG",
  debug: "DEBUG",
  info: "INFO",
  warn: "WARNING",
  error: "ERROR",
  fatal: "CRITICAL",
};

/**
 * pino renders this once per level when the logger is constructed (genLsCache)
 * and caches the resulting string prefix, so it must be a pure function of the
 * label — anything read here that can change per line would be frozen at the
 * first call.
 *
 * Returning `severity` in place of pino's numeric `level` is what makes the
 * line Cloud Logging shaped; the number is dropped entirely.
 */
const toSeverity = (label: string): { readonly severity: Severity } => ({
  // DEFAULT rather than a throw: an unmapped custom level should still be
  // visible in the Logs Explorer, just unranked.
  //
  // `Object.hasOwn` rather than a bare lookup: a label like `constructor` or
  // `toString` would otherwise resolve to the inherited Object.prototype
  // member and emit a function as the severity.
  severity: Object.hasOwn(SEVERITY_BY_LABEL, label)
    ? (SEVERITY_BY_LABEL[label] ?? "DEFAULT")
    : "DEFAULT",
});

export { toSeverity };
