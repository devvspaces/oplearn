import { useState } from "react";
import { Link, createFileRoute } from "@tanstack/react-router";
import {
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Circle,
  ListTodo,
  MessageCircleQuestion,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/misc";
import { addDays, formatDay, todayYmd, weekStart } from "@/lib/dates";
import { formatDuration, formatTimestamp } from "@/lib/time";
import { cn } from "@/lib/utils";
import { Heatmap } from "@/features/study/Heatmap";
import { MonthView } from "@/features/study/MonthView";
import { useActivity, usePlanned, useStudySettings, useWeeklyReview } from "@/features/study/api";
import { anchorSearch } from "@/lib/anchors";
import { kindLabel } from "@/features/materials/api";

export const Route = createFileRoute("/planner")({ component: Planner });

function Planner() {
  const today = todayYmd();
  const [start, setStart] = useState(weekStart(today));
  const [view, setView] = useState<"week" | "month">("week");
  const end = addDays(start, 6);
  const { data: planned } = usePlanned(start, end);
  const { data: review, isLoading } = useWeeklyReview(start);
  const { data: activity } = useActivity(365);
  const { data: settings } = useStudySettings();
  const goal = settings?.dailyGoalMin ?? 45;
  const days = Array.from({ length: 7 }, (_, i) => addDays(start, i));
  const minutesByDay = new Map((review?.days ?? []).map((d) => [d.date, d.minutes ?? 0]));
  const todosByDay = new Map<string, NonNullable<typeof review>["overdueTodos"]>();
  for (const t of review?.overdueTodos ?? [])
    if (t.dueDate && t.dueDate >= start) todosByDay.set(t.dueDate, [...(todosByDay.get(t.dueDate) ?? []), t]);

  return (
    <div className="mx-auto max-w-6xl space-y-10 p-8">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">Planner</h1>
        <div role="radiogroup" aria-label="View" className="flex gap-1 rounded-lg bg-muted p-1">
          {(["week", "month"] as const).map((v) => (
            <button
              key={v}
              role="radio"
              aria-checked={view === v}
              onClick={() => setView(v)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium capitalize text-muted-foreground",
                view === v && "bg-background text-foreground shadow-sm",
              )}
            >
              {v}
            </button>
          ))}
        </div>
        <div className={cn("flex items-center gap-1", view !== "week" && "hidden")}>
          <Button
            variant="outline"
            size="icon"
            aria-label="Previous week"
            onClick={() => setStart(addDays(start, -7))}
          >
            <ChevronLeft />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setStart(weekStart(today))}
            disabled={start === weekStart(today)}
          >
            This week
          </Button>
          <Button
            variant="outline"
            size="icon"
            aria-label="Next week"
            onClick={() => setStart(addDays(start, 7))}
          >
            <ChevronRight />
          </Button>
          <span className="ml-2 text-sm text-muted-foreground">
            {formatDay(start, { day: "numeric", month: "short" })} –{" "}
            {formatDay(end, { day: "numeric", month: "short", year: "numeric" })}
          </span>
        </div>
      </header>

      {view === "month" && (
        <MonthView
          goal={goal}
          onPickDay={(d) => {
            setStart(weekStart(d));
            setView("week");
          }}
        />
      )}
      <section
        aria-label="Week"
        className={cn("grid grid-cols-1 gap-2 md:grid-cols-7", view !== "week" && "hidden")}
      >
        {days.map((d) => {
          const items = (planned ?? []).filter((p) => p.plannedDate === d);
          const todos = todosByDay.get(d) ?? [];
          const mins = minutesByDay.get(d) ?? 0;
          return (
            <div
              key={d}
              className={cn(
                "flex min-h-36 flex-col rounded-xl border bg-card p-2",
                d === today && "border-primary/60 ring-1 ring-primary/30",
              )}
            >
              <div className="mb-2 flex items-baseline justify-between">
                <span
                  className={cn(
                    "text-xs font-medium",
                    d === today ? "text-primary" : "text-muted-foreground",
                  )}
                >
                  {formatDay(d, { weekday: "short", day: "numeric" })}
                </span>
                {mins >= 1 && (
                  <span
                    className={cn(
                      "text-[10px] tabular-nums",
                      mins >= goal ? "text-success" : "text-muted-foreground",
                    )}
                  >
                    {Math.round(mins)}m
                  </span>
                )}
              </div>
              <ul className="space-y-1">
                {items.map((p) => (
                  <li key={p.lessonId}>
                    <Link
                      to="/lessons/$lessonId"
                      params={{ lessonId: p.lessonId }}
                      title={[p.docKind ? kindLabel(p.docKind) : null, p.courseTitle, formatDuration(p.durationSec)]
                        .filter(Boolean)
                        .join(" · ")}
                      className={cn(
                        "flex items-start gap-1 rounded-md px-1 py-0.5 text-xs hover:bg-accent",
                        p.done && "text-muted-foreground line-through",
                        !p.done && d < today && "text-warning",
                      )}
                    >
                      {p.done ? (
                        <CheckCircle2 className="mt-px size-3 shrink-0 text-success" />
                      ) : (
                        <Circle className="mt-px size-3 shrink-0" />
                      )}
                      <span className="line-clamp-2">{p.lessonTitle}</span>
                    </Link>
                  </li>
                ))}
                {todos.map((t) => (
                  <li key={t.id}>
                    <Link
                      to="/lessons/$lessonId"
                      params={{ lessonId: t.lessonId }}
                      search={anchorSearch(t)}
                      className="flex items-start gap-1 rounded-md px-1 py-0.5 text-xs text-warning hover:bg-accent"
                    >
                      <ListTodo className="mt-px size-3 shrink-0" />
                      <span className="line-clamp-2">{t.title || "To-do"}</span>
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </section>

      <section>
        <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Activity · last 12 months
        </h2>
        <div className="rounded-xl border bg-card p-4">
          {activity ? <Heatmap days={activity} goal={goal} /> : <Spinner />}
        </div>
      </section>

      <section>
        <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Weekly review
        </h2>
        {isLoading || !review ? (
          <Spinner />
        ) : (
          <div className="grid gap-4 md:grid-cols-2">
            <div className="rounded-xl border bg-card p-4">
              <p className="text-3xl font-semibold tabular-nums">
                {Math.round(review.minutes ?? 0)}{" "}
                <span className="text-base font-normal text-muted-foreground">min studied</span>
              </p>
              <p className="mt-1 text-sm text-muted-foreground">
                {review.lessonsCompleted.length} lesson{review.lessonsCompleted.length === 1 ? "" : "s"}{" "}
                completed · {review.days.filter((d) => (d.minutes ?? 0) >= goal).length}/7 days at goal
              </p>
              <ul className="mt-3 space-y-1 text-sm">
                {review.lessonsCompleted.slice(0, 8).map((l) => (
                  <li key={l.lessonId} className="flex items-center gap-2">
                    <CheckCircle2 className="size-3.5 shrink-0 text-success" />
                    <Link
                      to="/lessons/$lessonId"
                      params={{ lessonId: l.lessonId }}
                      className="truncate hover:underline"
                    >
                      {l.lessonTitle}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
            <div className="space-y-4 rounded-xl border bg-card p-4 text-sm">
              <ReviewList
                title="Open questions"
                icon={<MessageCircleQuestion className="size-3.5" />}
                items={review.openQuestions}
                empty="No open questions."
              />
              <ReviewList
                title="To-dos due"
                icon={<ListTodo className="size-3.5" />}
                items={review.overdueTodos}
                empty="Nothing due."
              />
              <div>
                <h3 className="mb-1 font-medium">Next week</h3>
                {review.nextWeek.length ? (
                  <p className="text-muted-foreground">
                    {review.nextWeek.length} planned lesson{review.nextWeek.length === 1 ? "" : "s"} ·{" "}
                    {formatDuration(review.nextWeek.reduce((s, p) => s + (p.durationSec ?? 0), 0))}
                  </p>
                ) : (
                  <p className="text-muted-foreground">Nothing planned yet.</p>
                )}
              </div>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}

function ReviewList({
  title,
  icon,
  items,
  empty,
}: {
  title: string;
  icon: React.ReactNode;
  items: Array<{
    id: string;
    lessonId: string;
    lessonTitle: string;
    title: string;
    startSec: number | null;
    dueDate: string | null;
  }>;
  empty: string;
}) {
  return (
    <div>
      <h3 className="mb-1 flex items-center gap-1.5 font-medium">
        {icon} {title} <span className="text-muted-foreground">({items.length})</span>
      </h3>
      {items.length === 0 ? (
        <p className="text-muted-foreground">{empty}</p>
      ) : (
        <ul className="space-y-1">
          {items.slice(0, 6).map((q) => (
            <li key={q.id} className="truncate">
              <Link
                to="/lessons/$lessonId"
                params={{ lessonId: q.lessonId }}
                search={q.startSec != null ? { t: q.startSec } : {}}
                className="hover:underline"
              >
                {q.title || "Untitled"}
              </Link>
              <span className="text-xs text-muted-foreground">
                {" "}
                · {q.lessonTitle}
                {q.startSec != null ? ` @ ${formatTimestamp(q.startSec)}` : ""}
                {q.dueDate ? ` · due ${q.dueDate}` : ""}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
