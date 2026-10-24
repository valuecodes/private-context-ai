import { useEffect, useState } from "react";

import { msUntilNextDay } from "~/features/conversations/date-groups";

/**
 * The current time, held in state so render stays pure, and replaced at each
 * local midnight so the date groups roll over while the sidebar sits idle.
 */
const useNow = (): Date => {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = setTimeout(() => setNow(new Date()), msUntilNextDay(now));
    return () => clearTimeout(timer);
  }, [now]);
  return now;
};

export { useNow };
