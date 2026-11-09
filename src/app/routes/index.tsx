import { Link, createFileRoute } from "@tanstack/react-router";
import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  Layers,
  ListTodo,
  MessageCircleQuestion,
  PlayCircle,
} from "lucide-react";
import { useCourses, mediaSrc } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { EmptyState, Progress, Spinner } from "@/components/ui/misc";
import { relativeTime, formatDuration } from "@/lib/time";
import { relativeDay, todayYmd } from "@/lib/dates";
import { GoalRing } from "@/features/study/GoalRing";
import { useToday } from "@/features/study/api";
import { cn } from "@/lib/utils";
import { KindIcon, kindLabel } from "@/features/materials/api";

export const Route = createFileRoute("/")({ component: Today });

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-muted-foreground">{children}</h2>
  );
}

function Today() {
  const { data } = useCourses();
  const { data: today, isLoading } = useToday();
  const continuing = (data ?? [])
    .filter((c) => c.course.status === "active" && c.nextLessonId)
    .sort((a, b) => (b.lastStudiedAt ?? "").localeCompare(a.lastStudiedAt ?? ""))
    .slice(0, 6);
  const todayStr = todayYmd();
  const planned = today?.planned ?? [];
  const greeting = (() => {
    const h = new Date().getHours();
    return h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
  })();

  return (
    <div className="mx-auto max-w-5xl space-y-10 p-8">
      <header className="flex flex-wrap items-center justify-between gap-6">
        <div>
          <h1 className="text-2xl font-semibold">{greeting}</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            {new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}
          </p>
        </div>
        {isLoading || !today ? (
          <Spinner />
        ) : (
          <GoalRing minutes={today.minutesToday ?? 0} goal={today.goalMin} streak={today.streak} />
        )}
      </header>

      {today &&
        (today.behind.length > 0 || today.dueCards + today.overdueTodos + today.openQuestions > 0) && (
          <div className="grid gap-3 sm:grid-cols-3">
            {today.behind.map((b) => (
              <Link
                key={b.courseId}
                to="/courses/$courseId"
                params={{ courseId: b.courseId }}
                className="flex items-center gap-3 rounded-xl border border-warning/40 bg-warning/10 p-3 text-sm hover:bg-warning/15 sm:col-span-3"
              >
                <AlertTriangle className="size-4 shrink-0 text-warning" aria-hidden />
                <span className="min-w-0 flex-1 truncate">
                  <span className="font-medium">{b.courseTitle}</span> is behind by {b.lessons} lesson
                  {b.lessons === 1 ? "" : "s"}
                </span>
                <span className="shrink-0 text-xs text-muted-foreground">Open plan to rebalance</span>
              </Link>
            ))}
            <Stat icon={<Layers />} label="Flashcards due" value={today.dueCards} to="/review" />
            <Stat
              icon={<ListTodo />}
              label="Overdue to-dos"
              value={today.overdueTodos}
              to="/notes/todos"
              warn
            />
            <Stat
              icon={<MessageCircleQuestion />}
              label="Open questions"
              value={today.openQuestions}
              to="/notes/questions"
            />
          </div>
        )}

      <section>
        <SectionTitle>Today's plan</SectionTitle>
        {planned.length === 0 ? (
          <p className="rounded-xl border border-dashed p-6 text-center text-sm text-muted-foreground">
            Nothing planned for today. Set a target on a course's <span className="font-medium">Plan</span>{" "}
            tab to get a daily schedule.
          </p>
        ) : (
          <ul className="divide-y rounded-xl border bg-card">
            {planned.map((p) => (
              <li key={p.lessonId} className="flex items-center gap-3 px-3 py-2.5">
                {p.done ? (
                  <CheckCircle2 className="size-4.5 shrink-0 text-success" aria-label="Done" />
                ) : (
                  <Circle className="size-4.5 shrink-0 text-muted-foreground" aria-label="Not done" />
                )}
                <div className="min-w-0 flex-1">
                  <Link
                    to="/lessons/$lessonId"
                    params={{ lessonId: p.lessonId }}
                    className={cn(
                      "block truncate text-sm hover:underline",
                      p.done && "text-muted-foreground line-through",
                    )}
                  >
                    {p.lessonTitle}
                  </Link>
                  <span className="flex items-center gap-1 text-xs text-muted-foreground">
                    {p.docKind && (
                      <>
                        <KindIcon kind={p.docKind} className="size-3" />
                        {kindLabel(p.docKind)} ·
                      </>
                    )}
                    {p.courseTitle}
                  </span>
                </div>
                {p.plannedDate < todayStr && !p.done && (
                  <span className="shrink-0 text-xs text-warning">from {relativeDay(p.plannedDate)}</span>
                )}
                <span className="w-14 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
                  {formatDuration(p.durationSec)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <SectionTitle>Continue learning</SectionTitle>
        {continuing.length === 0 ? (
          <EmptyState icon={<PlayCircle className="size-8" />} title="Nothing in progress yet">
            <Button asChild className="mt-2">
              <Link to="/library">Go to Library</Link>
            </Button>
          </EmptyState>
        ) : (
          <div className="grid gap-3 sm:grid-cols-2">
            {continuing.map((c) => (
              <Link
                key={c.course.id}
                to="/lessons/$lessonId"
                params={{ lessonId: c.nextLessonId! }}
                className="flex gap-3 rounded-xl border bg-card p-3 transition-colors hover:bg-accent"
              >
                {c.course.coverPath ? (
                  <img
                    src={mediaSrc(c.course.coverPath)}
                    alt=""
                    className="h-16 w-28 rounded-md object-cover"
                  />
                ) : (
                  <div className="h-16 w-28 rounded-md bg-muted" />
                )}
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium">{c.course.title}</div>
                  <div className="truncate text-xs text-muted-foreground">Next: {c.nextLessonTitle}</div>
                  <Progress
                    className="mt-2"
                    value={c.lessonCount ? (100 * c.completedCount) / c.lessonCount : 0}
                    label="Course progress"
                  />
                  <div className="mt-1 text-[11px] text-muted-foreground">
                    {c.completedCount}/{c.lessonCount} lessons{" "}
                    {c.lastStudiedAt && `· ${relativeTime(c.lastStudiedAt)}`}
                  </div>
                </div>
              </Link>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function Stat({
  icon,
  label,
  value,
  to,
  warn,
}: {
  icon: React.ReactNode;
  label: string;
  value: number;
  to: string;
  warn?: boolean;
}) {
  return (
    <Link
      to={to}
      className="flex items-center gap-3 rounded-xl border bg-card p-3 hover:bg-accent [&_svg]:size-4.5 [&_svg]:text-muted-foreground"
    >
      {icon}
      <span className="flex-1 text-sm text-muted-foreground">{label}</span>
      <span className={cn("text-lg font-semibold tabular-nums", warn && value > 0 && "text-warning")}>
        {value}
      </span>
    </Link>
  );
}
