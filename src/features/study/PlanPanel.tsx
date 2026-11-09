import { useEffect, useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { CalendarClock, CheckCircle2, Circle, Scale, Sparkles } from "lucide-react";
import { AiPlanDialog } from "./AiPlanDialog";
import type { Target } from "@/bindings";
import { Button } from "@/components/ui/button";
import { Input, Label } from "@/components/ui/input";
import { Spinner } from "@/components/ui/misc";
import { addDays, relativeDay, todayYmd } from "@/lib/dates";
import { formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import { useCoursePlan, useRebalance, useSetPlan, useSetPlanOrder } from "./api";
import { PlanStatusChip } from "./PlanStatusChip";

type Kind = Target["kind"] | "none";

const KINDS: Array<[Kind, string]> = [
  ["date", "Finish by a date"],
  ["lessonsPerWeek", "Lessons per week"],
  ["minutesPerWeek", "Hours per week"],
  ["none", "No plan"],
];

/** Course Plan tab: choose a target, see the generated schedule and on-track status. */
export function PlanPanel({ courseId }: { courseId: string }) {
  const { data: plan, isLoading } = useCoursePlan(courseId);
  const setPlan = useSetPlan(courseId);
  const rebalance = useRebalance(courseId);
  const setOrder = useSetPlanOrder(courseId);
  const [aiOpen, setAiOpen] = useState(false);
  const [kind, setKind] = useState<Kind>("date");
  const [date, setDate] = useState(addDays(todayYmd(), 28));
  const [perWeek, setPerWeek] = useState("3");
  const [hours, setHours] = useState("5");

  useEffect(() => {
    const t = plan?.target;
    if (!t) return;
    setKind(t.kind);
    if (t.kind === "date") setDate(t.date);
    if (t.kind === "lessonsPerWeek") setPerWeek(String(t.value));
    if (t.kind === "minutesPerWeek") setHours(String(Math.round((t.value / 60) * 10) / 10));
  }, [plan?.target]);

  const byDay = useMemo(() => {
    const m = new Map<string, NonNullable<typeof plan>["items"]>();
    for (const i of plan?.items ?? []) m.set(i.plannedDate, [...(m.get(i.plannedDate) ?? []), i]);
    return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [plan]);

  function target(): Target | null {
    if (kind === "date") return { kind, date };
    if (kind === "lessonsPerWeek") return { kind, value: Math.max(1, Math.round(Number(perWeek) || 1)) };
    if (kind === "minutesPerWeek")
      return { kind, value: Math.max(15, Math.round((Number(hours) || 1) * 60)) };
    return null;
  }

  if (isLoading) return <Spinner />;
  const today = todayYmd();

  return (
    <div className="space-y-6">
      <form
        className="rounded-xl border bg-card p-4"
        onSubmit={(e) => {
          e.preventDefault();
          setPlan.mutate(target());
        }}
      >
        <div
          role="radiogroup"
          aria-label="Plan type"
          className="flex flex-wrap gap-1 rounded-lg bg-muted p-1 w-fit"
        >
          {KINDS.map(([k, label]) => (
            <button
              key={k}
              type="button"
              role="radio"
              aria-checked={kind === k}
              onClick={() => setKind(k)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium text-muted-foreground",
                kind === k && "bg-background text-foreground shadow-sm",
              )}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="mt-4 flex flex-wrap items-end gap-3">
          {kind === "date" && (
            <div className="grid gap-1">
              <Label htmlFor="plan-date">Target finish date</Label>
              <Input
                id="plan-date"
                type="date"
                min={today}
                value={date}
                onChange={(e) => setDate(e.target.value)}
                className="w-44"
              />
            </div>
          )}
          {kind === "lessonsPerWeek" && (
            <div className="grid gap-1">
              <Label htmlFor="plan-lessons">Lessons per week</Label>
              <Input
                id="plan-lessons"
                type="number"
                min={1}
                max={50}
                value={perWeek}
                onChange={(e) => setPerWeek(e.target.value)}
                className="w-28"
              />
            </div>
          )}
          {kind === "minutesPerWeek" && (
            <div className="grid gap-1">
              <Label htmlFor="plan-hours">Hours per week</Label>
              <Input
                id="plan-hours"
                type="number"
                min={0.5}
                step={0.5}
                max={60}
                value={hours}
                onChange={(e) => setHours(e.target.value)}
                className="w-28"
              />
            </div>
          )}
          <Button type="submit" disabled={setPlan.isPending}>
            <CalendarClock /> {kind === "none" ? "Remove plan" : plan?.target ? "Update plan" : "Create plan"}
          </Button>
          <Button type="button" variant="outline" onClick={() => setAiOpen(true)}>
            <Sparkles /> Plan with AI
          </Button>
          {plan?.target && (
            <Button
              type="button"
              variant="outline"
              disabled={rebalance.isPending}
              onClick={() => rebalance.mutate()}
            >
              <Scale /> Rebalance from today
            </Button>
          )}
        </div>
        <p className="mt-3 text-xs text-muted-foreground">
          Lessons are spread across your study days (Settings → Study). Rebalancing re-spreads everything not
          yet completed, starting today.
        </p>
        {plan?.order === "custom" && plan.items.length > 0 && (
          <p role="status" className="mt-2 flex flex-wrap items-center gap-2 text-xs">
            <Sparkles className="size-3.5 text-primary" aria-hidden />
            <span>Following the AI plan's order. Rebalancing keeps it; new lessons go at the end.</span>
            <button
              type="button"
              className="text-primary hover:underline disabled:opacity-50"
              disabled={setOrder.isPending}
              onClick={() => setOrder.mutate("outline")}
            >
              Use outline order
            </button>
          </p>
        )}
      </form>

      {plan && plan.items.length > 0 && (
        <div>
          <div className="mb-3 flex items-center gap-2 text-sm">
            <PlanStatusChip status={plan.status} />
            {plan.finishDate && (
              <span className="text-muted-foreground">Finishes {relativeDay(plan.finishDate)}</span>
            )}
          </div>
          <ol className="space-y-3">
            {byDay.map(([day, items]) => {
              const overdue = day < today && items.some((i) => !i.done);
              return (
                <li key={day} className="grid grid-cols-[7rem_1fr] gap-3">
                  <span
                    className={cn(
                      "pt-1 text-xs font-medium",
                      day === today ? "text-primary" : overdue ? "text-warning" : "text-muted-foreground",
                    )}
                  >
                    {relativeDay(day)}
                  </span>
                  <ul className="space-y-1">
                    {items.map((i) => (
                      <li key={i.lessonId} className="flex items-center gap-2 text-sm">
                        {i.done ? (
                          <CheckCircle2 className="size-4 text-success" aria-label="Done" />
                        ) : (
                          <Circle className="size-4 text-muted-foreground" aria-label="Not done" />
                        )}
                        <Link
                          to="/lessons/$lessonId"
                          params={{ lessonId: i.lessonId }}
                          className={cn("truncate hover:underline", i.done && "text-muted-foreground")}
                        >
                          {i.lessonTitle}
                        </Link>
                        <span className="ml-auto shrink-0 text-xs tabular-nums text-muted-foreground">
                          {formatDuration(i.durationSec)}
                        </span>
                      </li>
                    ))}
                  </ul>
                </li>
              );
            })}
          </ol>
        </div>
      )}
      <AiPlanDialog courseId={courseId} open={aiOpen} onOpenChange={setAiOpen} />
    </div>
  );
}
