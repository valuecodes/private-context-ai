import { useEffect, useState } from "react";

import type { StatusState } from "~/features/status/status-state";
import { loadOllamaStatus } from "~/features/status/status-state";

/** Checks Ollama once on mount and again on every `retry`. */
const useOllamaStatus = () => {
  const [status, setStatus] = useState<StatusState>({ phase: "checking" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void loadOllamaStatus().then((next) => {
      if (!cancelled) {
        setStatus(next);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  const retry = () => {
    setStatus({ phase: "checking" });
    setAttempt((count) => count + 1);
  };

  return { status, retry };
};

export { useOllamaStatus };
