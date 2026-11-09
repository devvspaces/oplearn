import { useMemo, useState } from "react";
import { ChevronLeft, ChevronRight, ListTodo } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { commands } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { addMonths, formatDay, heatLevel, monthGrid, sameMonth, todayYmd } from "@/lib/dates";
import { cn } from "@/lib/utils";
import { useActivity, usePlanned } from "./api";
import { KindIcon, kindLabel } from "@/features/materials/api";

const HEAT_BG = [
  "",
  "color-mix(in oklab, var(--primary) 10%, transparent)",
  "color-mix(in oklab, var(--primary) 18%, transparent)",
  "color-mix(in oklab, var(--primary) 26%, transparent)",
  "color-mix(in oklab, var(--primary) 36%, transparent)",
];
const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** Month calendar: planned lessons, to-do due dates and minutes studied (cell tint). */
export function MonthView({ goal, onPickDay }: { goal: number; onPickDay: (ymd: string) => void }) {
  const today = todayYmd();
  const [month, setMonth] = useState(today);
  const grid = useMemo(() => monthGrid(month), [month]);
  const from = grid[0]![0]!;
  const to = grid[5]![6]!;
  const { data: planned } = usePlanned(from, to);
  const { data: todos } = useQuery({
    queryKey: ["study", "todos", from, to],
    queryFn: () => commands.getTodosBetween(from, to),
  });
  const { data: activity } = useActivity(400);
  const minutes = useMemo(() => new Map((activity ?? []).map((d) => [d.date, d.minutes ?? 0])), [activity]);

  return (
    <section aria-label="Month">
      <div className="mb-2 flex items-center gap-1">
        <Button
          variant="outline"
          size="icon"
          aria-label="Previous month"
          onClick={() => setMonth(addMonths(month, -1))}
        >
          <ChevronLeft />
        </Button>
        <Button variant="ghost" size="sm" onClick={() => setMonth(today)} disabled={sameMonth(month, today)}>
          This month
        </Button>
        <Button
          variant="outline"
          size="icon"
          aria-label="Next month"
          onClick={() => setMonth(addMonths(month, 1))}
        >
          <ChevronRight />
        </Button>
        <h2 className="ml-2 text-sm font-medium">{formatDay(month, { month: "long", year: "numeric" })}</h2>
      </div>
      <div className="grid grid-cols-7 gap-1 text-[11px] text-muted-foreground">
        {WEEKDAYS.map((d) => (
          <div key={d} className="px-1 pb-1">
            {d}
          </div>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-1">
        {grid.flat().map((d) => {
          const items = (planned ?? []).filter((p) => p.plannedDate === d);
          const due = (todos ?? []).filter((t) => t.dueDate === d);
          const mins = minutes.get(d) ?? 0;
          const inMonth = sameMonth(d, month);
          return (
            <button
              key={d}
              onClick={() => onPickDay(d)}
              aria-label={`${formatDay(d, { weekday: "long", day: "numeric", month: "long" })}: ${items.length} planned, ${Math.round(mins)} minutes studied`}
              className={cn(
                "flex min-h-24 flex-col rounded-lg border p-1.5 text-left transition-colors hover:border-primary/60",
                !inMonth && "opacity-45",
                d === today && "border-primary ring-1 ring-primary/30",
              )}
              style={{ background: HEAT_BG[heatLevel(mins, goal)] || undefined }}
            >
              <span className="flex items-center justify-between text-[11px]">
                <span className={cn("font-medium", d === today ? "text-primary" : "text-muted-foreground")}>
                  {Number(d.slice(8))}
                </span>
                {mins >= 1 && (
                  <span
                    className={cn("tabular-nums", mins >= goal ? "text-success" : "text-muted-foreground")}
                  >
                    {Math.round(mins)}m
                  </span>
                )}
              </span>
              <ul className="mt-1 space-y-0.5">
                {items.slice(0, 3).map((p) => (
                  <li
                    key={p.lessonId}
                    className={cn(
                      "truncate text-[11px]",
                      p.done ? "text-muted-foreground line-through" : d < today ? "text-warning" : "",
                    )}
                    title={[p.lessonTitle, p.docKind ? kindLabel(p.docKind) : null, p.courseTitle]
                      .filter(Boolean)
                      .join(" · ")}
                  >
                    {p.docKind && <KindIcon kind={p.docKind} className="mr-0.5 inline size-3 align-[-2px]" />}
                    {p.lessonTitle}
                  </li>
                ))}
                {items.length > 3 && (
                  <li className="text-[10px] text-muted-foreground">+{items.length - 3} more</li>
                )}
                {due.length > 0 && (
                  <li className="flex items-center gap-0.5 text-[10px] text-warning">
                    <ListTodo className="size-3" /> {due.length} due
                  </li>
                )}
              </ul>
            </button>
          );
        })}
      </div>
    </section>
  );
}
