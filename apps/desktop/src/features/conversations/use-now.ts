import { useEffect, useState } from "react";

import { msUntilNextDay } from "~/features/conversations/date-groups";

/**
 * The current time, held in state so render stays pure, and replaced at each
 * local midnight so the date groups roll over while the sidebar sits idle.
 * Window focus also replaces it: timers can stall while the OS sleeps, so
 * waking past midnight would otherwise keep yesterday's groups.
 */
const useNow = (): Date => {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = setTimeout(() => setNow(new Date()), msUntilNextDay(now));
    return () => clearTimeout(timer);
  }, [now]);
  useEffect(() => {
    const refresh = () => setNow(new Date());
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);
  return now;
};

export { useNow };
