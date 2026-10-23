import { Check, Copy, TriangleAlert } from "lucide-react";
import { useId, useState } from "react";

import { copyText } from "~/lib/clipboard";
import type { ChartSpec } from "~/lib/types";

const COPIED_MS = 1500;

/**
 * Fixed colours, readable on light and dark backgrounds. Whole class names,
 * so Tailwind finds them; nothing here is built from model text.
 */
const PALETTE = [
  {
    fill: "fill-sky-500 dark:fill-sky-400",
    stroke: "stroke-sky-500 dark:stroke-sky-400",
    swatch: "bg-sky-500 dark:bg-sky-400",
  },
  {
    fill: "fill-amber-500 dark:fill-amber-400",
    stroke: "stroke-amber-500 dark:stroke-amber-400",
    swatch: "bg-amber-500 dark:bg-amber-400",
  },
  {
    fill: "fill-emerald-500 dark:fill-emerald-400",
    stroke: "stroke-emerald-500 dark:stroke-emerald-400",
    swatch: "bg-emerald-500 dark:bg-emerald-400",
  },
  {
    fill: "fill-rose-500 dark:fill-rose-400",
    stroke: "stroke-rose-500 dark:stroke-rose-400",
    swatch: "bg-rose-500 dark:bg-rose-400",
  },
  {
    fill: "fill-violet-500 dark:fill-violet-400",
    stroke: "stroke-violet-500 dark:stroke-violet-400",
    swatch: "bg-violet-500 dark:bg-violet-400",
  },
  {
    fill: "fill-teal-500 dark:fill-teal-400",
    stroke: "stroke-teal-500 dark:stroke-teal-400",
    swatch: "bg-teal-500 dark:bg-teal-400",
  },
  {
    fill: "fill-orange-500 dark:fill-orange-400",
    stroke: "stroke-orange-500 dark:stroke-orange-400",
    swatch: "bg-orange-500 dark:bg-orange-400",
  },
  {
    fill: "fill-indigo-500 dark:fill-indigo-400",
    stroke: "stroke-indigo-500 dark:stroke-indigo-400",
    swatch: "bg-indigo-500 dark:bg-indigo-400",
  },
  {
    fill: "fill-lime-500 dark:fill-lime-400",
    stroke: "stroke-lime-500 dark:stroke-lime-400",
    swatch: "bg-lime-500 dark:bg-lime-400",
  },
  {
    fill: "fill-pink-500 dark:fill-pink-400",
    stroke: "stroke-pink-500 dark:stroke-pink-400",
    swatch: "bg-pink-500 dark:bg-pink-400",
  },
  {
    fill: "fill-cyan-500 dark:fill-cyan-400",
    stroke: "stroke-cyan-500 dark:stroke-cyan-400",
    swatch: "bg-cyan-500 dark:bg-cyan-400",
  },
  {
    fill: "fill-zinc-500 dark:fill-zinc-400",
    stroke: "stroke-zinc-500 dark:stroke-zinc-400",
    swatch: "bg-zinc-500 dark:bg-zinc-400",
  },
] as const;

const colour = (index: number) => PALETTE[index % PALETTE.length] ?? PALETTE[0];

const WIDTH = 480;
const HEIGHT = 260;
const LEFT = 52;
const RIGHT = 12;
const TOP = 12;
const BOTTOM = 40;
const PLOT_WIDTH = WIDTH - LEFT - RIGHT;
const PLOT_HEIGHT = HEIGHT - TOP - BOTTOM;
/** Past this many points, only every n-th label is shown under the axis. */
const MAX_AXIS_LABELS = 12;
const MAX_LABEL_CHARS = 12;

const numberFormat = new Intl.NumberFormat(undefined, {
  maximumFractionDigits: 2,
});
const axisFormat = new Intl.NumberFormat(undefined, {
  maximumFractionDigits: 1,
  notation: "compact",
});

/** A coordinate, short in the markup. */
const at = (value: number): string => value.toFixed(1);

const shortLabel = (label: string): string =>
  label.length > MAX_LABEL_CHARS
    ? `${label.slice(0, MAX_LABEL_CHARS - 1)}…`
    : label;

/**
 * Value to `y`. The range always spans zero, and a zero span (all values 0)
 * counts as 1, so nothing divides by zero. Rust caps values at 1e15.
 */
const scaleOf = (values: number[]) => {
  const low = Math.min(0, ...values);
  const high = Math.max(0, ...values);
  const span = high - low === 0 ? 1 : high - low;
  return {
    low,
    high,
    y: (value: number) => TOP + ((high - value) / span) * PLOT_HEIGHT,
  };
};

type Scale = ReturnType<typeof scaleOf>;

/** The value axis: the top, zero and bottom of the range. */
const ValueAxis = ({ scale }: { scale: Scale }) => {
  const ticks = [...new Set([scale.high, 0, scale.low])];
  return (
    <g className="fill-zinc-500 text-[10px] dark:fill-zinc-400">
      {ticks.map((tick) => (
        <text
          key={tick}
          x={LEFT - 6}
          y={at(scale.y(tick) + 3)}
          textAnchor="end"
        >
          {axisFormat.format(tick)}
        </text>
      ))}
      <line
        x1={LEFT}
        x2={WIDTH - RIGHT}
        y1={at(scale.y(0))}
        y2={at(scale.y(0))}
        className="stroke-zinc-300 dark:stroke-zinc-700"
      />
    </g>
  );
};

/** Labels under the plot, at `xs`; every n-th when there are many. */
const CategoryAxis = ({ labels, xs }: { labels: string[]; xs: number[] }) => {
  const every = Math.ceil(labels.length / MAX_AXIS_LABELS);
  return (
    <g className="fill-zinc-500 text-[10px] dark:fill-zinc-400">
      {labels.map((label, index) =>
        index % every === 0 ? (
          <text
            // Labels may repeat; the position is unique.
            key={index}
            x={at(xs[index] ?? 0)}
            y={HEIGHT - BOTTOM + 16}
            textAnchor="middle"
          >
            {shortLabel(label)}
          </text>
        ) : null
      )}
    </g>
  );
};

const pointTitle = (label: string, value: number): string =>
  `${label}: ${numberFormat.format(value)}`;

const Bars = ({ spec }: { spec: ChartSpec }) => {
  const scale = scaleOf(spec.values);
  const slot = PLOT_WIDTH / spec.values.length;
  const xs = spec.values.map((_, index) => LEFT + (index + 0.5) * slot);
  const zero = scale.y(0);
  return (
    <>
      <ValueAxis scale={scale} />
      {spec.values.map((value, index) => {
        const y = scale.y(value);
        return (
          <rect
            key={index}
            x={at((xs[index] ?? 0) - slot * 0.35)}
            y={at(Math.min(y, zero))}
            width={at(slot * 0.7)}
            height={at(Math.abs(zero - y))}
            className={colour(0).fill}
          >
            <title>{pointTitle(spec.labels[index] ?? "", value)}</title>
          </rect>
        );
      })}
      <CategoryAxis labels={spec.labels} xs={xs} />
    </>
  );
};

const Line = ({ spec }: { spec: ChartSpec }) => {
  const scale = scaleOf(spec.values);
  const count = spec.values.length;
  const xs = spec.values.map((_, index) =>
    count === 1
      ? LEFT + PLOT_WIDTH / 2
      : LEFT + (index * PLOT_WIDTH) / (count - 1)
  );
  const points = spec.values
    .map((value, index) => `${at(xs[index] ?? 0)},${at(scale.y(value))}`)
    .join(" ");
  return (
    <>
      <ValueAxis scale={scale} />
      <polyline
        points={points}
        fill="none"
        strokeWidth={2}
        className={colour(0).stroke}
      />
      {spec.values.map((value, index) => (
        <circle
          key={index}
          cx={at(xs[index] ?? 0)}
          cy={at(scale.y(value))}
          r={3.5}
          className={colour(0).fill}
        >
          <title>{pointTitle(spec.labels[index] ?? "", value)}</title>
        </circle>
      ))}
      <CategoryAxis labels={spec.labels} xs={xs} />
    </>
  );
};

const PIE_RADIUS = 110;
const PIE_X = WIDTH / 2;
const PIE_Y = HEIGHT / 2;

const pointOn = (angle: number) => ({
  x: PIE_X + PIE_RADIUS * Math.sin(angle),
  y: PIE_Y - PIE_RADIUS * Math.cos(angle),
});

/** Slices clockwise from the top. Rust guarantees a positive total. */
const Pie = ({ spec }: { spec: ChartSpec }) => {
  const total = spec.values.reduce((sum, value) => sum + value, 0);
  const shown = spec.values.filter((value) => value > 0).length;
  const turn = (value: number) =>
    total > 0 ? (value / total) * 2 * Math.PI : 0;
  // Each slice starts where the values before it end.
  const starts = spec.values.map((_, index) =>
    turn(spec.values.slice(0, index).reduce((sum, value) => sum + value, 0))
  );
  return (
    <>
      {spec.values.map((value, index) => {
        const label = spec.labels[index] ?? "";
        const angle = turn(value);
        const from = starts[index] ?? 0;
        if (value <= 0) {
          return null;
        }
        const title = <title>{pointTitle(label, value)}</title>;
        if (shown === 1) {
          return (
            <circle
              key={index}
              cx={PIE_X}
              cy={PIE_Y}
              r={PIE_RADIUS}
              className={colour(index).fill}
            >
              {title}
            </circle>
          );
        }
        const a = pointOn(from);
        const b = pointOn(from + angle);
        const large = angle > Math.PI ? 1 : 0;
        const path = `M ${at(PIE_X)} ${at(PIE_Y)} L ${at(a.x)} ${at(a.y)} A ${PIE_RADIUS} ${PIE_RADIUS} 0 ${large} 1 ${at(b.x)} ${at(b.y)} Z`;
        return (
          <path
            key={index}
            d={path}
            className={`${colour(index).fill} stroke-white dark:stroke-zinc-950`}
          >
            {title}
          </path>
        );
      })}
    </>
  );
};

const Legend = ({ spec }: { spec: ChartSpec }) => (
  <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-zinc-600 dark:text-zinc-300">
    {spec.labels.map((label, index) => (
      <li key={index} className="flex items-center gap-1.5">
        <span
          aria-hidden="true"
          className={`inline-block size-2.5 rounded-sm ${colour(index).swatch}`}
        />
        {label}
        <span className="text-zinc-500 dark:text-zinc-400">
          {numberFormat.format(spec.values[index] ?? 0)}
        </span>
      </li>
    ))}
  </ul>
);

/** The chart's data as a table, for screen readers and exact values. */
const ChartTable = ({ spec }: { spec: ChartSpec }) => (
  <table className="w-full text-left text-xs">
    <caption className="sr-only">{spec.title}</caption>
    <thead>
      <tr className="border-b border-zinc-200 dark:border-zinc-800">
        <th scope="col" className="py-1 pr-4 font-medium">
          Label
        </th>
        <th scope="col" className="py-1 text-right font-medium">
          Value
        </th>
      </tr>
    </thead>
    <tbody>
      {spec.labels.map((label, index) => (
        <tr
          key={index}
          className="border-b border-zinc-100 dark:border-zinc-900"
        >
          <td className="py-1 pr-4">{label}</td>
          <td className="py-1 text-right tabular-nums">
            {numberFormat.format(spec.values[index] ?? 0)}
          </td>
        </tr>
      ))}
    </tbody>
  </table>
);

const headerButton =
  "flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-zinc-200 dark:hover:bg-zinc-800";

/**
 * A chart the model asked for, drawn as SVG by this code from Rust-validated
 * numbers. Model text reaches the DOM only as text nodes: no `href`, no
 * `style`, no `foreignObject`, `image` or `use`.
 */
const ChartBlock = ({ spec }: { spec: ChartSpec }) => {
  const [showData, setShowData] = useState(false);
  const [copied, setCopied] = useState(false);
  const titleId = useId();

  const copy = () => {
    void copyText(JSON.stringify(spec, null, 2)).then((ok) => {
      if (ok) {
        setCopied(true);
        setTimeout(() => {
          setCopied(false);
        }, COPIED_MS);
      }
    });
  };

  return (
    <figure className="my-3 overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-800">
      <div className="flex items-center justify-between gap-2 bg-zinc-100 px-3 py-1 text-xs text-zinc-500 dark:bg-zinc-900 dark:text-zinc-400">
        <figcaption
          id={titleId}
          className="truncate font-medium text-zinc-700 dark:text-zinc-200"
        >
          {spec.title === "" ? "Chart" : spec.title}
        </figcaption>
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            aria-pressed={showData}
            onClick={() => {
              setShowData((shown) => !shown);
            }}
            className={headerButton}
          >
            Show data
          </button>
          <button
            type="button"
            onClick={copy}
            aria-label={copied ? "Copied" : "Copy chart data"}
            title="Copy chart data as JSON"
            className={headerButton}
          >
            {copied ? (
              <Check aria-hidden="true" className="size-3.5" />
            ) : (
              <Copy aria-hidden="true" className="size-3.5" />
            )}
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      </div>
      <div className="bg-white p-3 dark:bg-zinc-950">
        {showData ? (
          <ChartTable spec={spec} />
        ) : (
          <>
            <svg
              role="img"
              aria-labelledby={titleId}
              viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
              className="h-auto w-full"
            >
              {spec.type === "bar" && <Bars spec={spec} />}
              {spec.type === "line" && <Line spec={spec} />}
              {spec.type === "pie" && <Pie spec={spec} />}
            </svg>
            {spec.type === "pie" && <Legend spec={spec} />}
          </>
        )}
      </div>
    </figure>
  );
};

/** A `render_chart` call Rust refused; `reason` is Rust's fixed text. */
const ChartFailed = ({ reason }: { reason: string | undefined }) => (
  <p
    title={reason}
    className="my-2 flex items-center gap-1.5 text-xs text-zinc-500 dark:text-zinc-400"
  >
    <TriangleAlert aria-hidden="true" className="size-3.5" />
    Chart could not be shown
  </p>
);

export { ChartBlock, ChartFailed, ChartTable };
