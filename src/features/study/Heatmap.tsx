import { useState } from "react";
import type { DayActivity } from "@/bindings";
import { formatDay, heatLevel, toWeeks } from "@/lib/dates";

const CELL = 11;
const GAP = 2;
/** Sequential, one hue: primary at increasing strength over the muted surface. */
const FILL = [
  "var(--muted)",
  "color-mix(in oklab, var(--primary) 30%, var(--muted))",
  "color-mix(in oklab, var(--primary) 55%, var(--muted))",
  "color-mix(in oklab, var(--primary) 80%, var(--muted))",
  "var(--primary)",
];

/** GitHub-style activity heatmap: one cell per day, columns are weeks (Monday first). */
export function Heatmap({ days, goal }: { days: DayActivity[]; goal: number }) {
  const weeks = toWeeks(days);
  const [hover, setHover] = useState<{ d: DayActivity; x: number; y: number } | null>(null);
  const width = weeks.length * (CELL + GAP);
  const height = 7 * (CELL + GAP);
  const active = days.filter((d) => (d.minutes ?? 0) >= 1).length;
  const total = Math.round(days.reduce((s, d) => s + (d.minutes ?? 0), 0));
  const metGoal = days.filter((d) => (d.minutes ?? 0) >= goal).length;

  return (
    <figure className="relative">
      <div className="overflow-x-auto pb-1">
        <svg
          width={width}
          height={height}
          role="img"
          aria-label={`Study activity: ${active} active days, ${total} minutes, goal met on ${metGoal} days`}
        >
          {weeks.map((w, wi) =>
            w.map((d, di) =>
              d ? (
                <rect
                  key={d.date}
                  x={wi * (CELL + GAP)}
                  y={di * (CELL + GAP)}
                  width={CELL}
                  height={CELL}
                  rx={2}
                  fill={FILL[heatLevel(d.minutes ?? 0, goal)]}
                  onMouseEnter={(e) => {
                    const r = (e.currentTarget.ownerSVGElement as SVGSVGElement).getBoundingClientRect();
                    const box = (e.currentTarget as SVGRectElement).getBoundingClientRect();
                    setHover({ d, x: box.left - r.left + CELL / 2, y: box.top - r.top });
                  }}
                  onMouseLeave={() => setHover(null)}
                >
                  <title>{`${formatDay(d.date)}: ${Math.round(d.minutes ?? 0)} min`}</title>
                </rect>
              ) : null,
            ),
          )}
        </svg>
      </div>
      {hover && (
        <div
          className="pointer-events-none absolute z-10 -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-md border bg-popover px-2 py-1 text-xs shadow-lg"
          style={{ left: hover.x, top: hover.y - 4 }}
        >
          <span className="font-medium tabular-nums">{Math.round(hover.d.minutes ?? 0)} min</span>
          <span className="text-muted-foreground">
            {" "}
            · {formatDay(hover.d.date, { weekday: "short", day: "numeric", month: "short", year: "numeric" })}
          </span>
          {(hover.d.minutes ?? 0) >= goal && <span className="text-muted-foreground"> · goal met</span>}
        </div>
      )}
      <figcaption className="mt-2 flex items-center justify-between gap-4 text-[11px] text-muted-foreground">
        <span>
          {total} min over {active} days · goal met {metGoal}×
        </span>
        <span className="flex items-center gap-1" aria-hidden>
          Less
          {FILL.map((f, i) => (
            <svg key={i} width={CELL} height={CELL}>
              <rect width={CELL} height={CELL} rx={2} fill={f} />
            </svg>
          ))}
          More
        </span>
      </figcaption>
    </figure>
  );
}
