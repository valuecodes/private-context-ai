import { formatDuration } from "~/features/activity/activity";
import type { ChartSpec, ModelStats } from "~/lib/types";

/** Most bars a `ChartSpec` takes. */
const MAX_BARS = 100;

/** Fastest median generation speed first, then by name. */
const sortBySpeed = (stats: ModelStats[]): ModelStats[] =>
  stats.toSorted(
    (a, b) => b.genTpsMedian - a.genTpsMedian || a.model.localeCompare(b.model)
  );

/** Tokens per second to one decimal, or an em dash when unknown. */
const formatTps = (tps: number | null): string =>
  tps === null ? "—" : tps.toFixed(1);

/** A median load time, or an em dash when unknown. */
const formatLoad = (ms: number | null): string =>
  ms === null ? "—" : formatDuration(Math.round(ms));

/** A bar per model, in the order given, of its median generation speed. */
const speedChart = (stats: ModelStats[]): ChartSpec => {
  const shown = stats.slice(0, MAX_BARS);
  return {
    title: "Median generation speed (tok/s)",
    type: "bar",
    labels: shown.map((entry) => entry.model),
    values: shown.map((entry) => Math.round(entry.genTpsMedian * 10) / 10),
  };
};

export { formatLoad, formatTps, sortBySpeed, speedChart };
