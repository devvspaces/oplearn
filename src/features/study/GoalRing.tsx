import { Flame, Snowflake } from "lucide-react";
import type { Streak } from "@/bindings";

/** Daily goal ring (minutes today vs goal) with the streak beside it. */
export function GoalRing({
  minutes,
  goal,
  streak,
  size = 112,
}: {
  minutes: number;
  goal: number;
  streak: Streak;
  size?: number;
}) {
  const stroke = 10;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const v = Math.max(0, Math.min(1, minutes / Math.max(1, goal)));
  const met = minutes >= goal;
  return (
    <div className="flex items-center gap-5">
      <div className="relative" style={{ width: size, height: size }}>
        <svg
          width={size}
          height={size}
          className="-rotate-90"
          role="img"
          aria-label={`${Math.floor(minutes)} of ${goal} minutes studied today`}
        >
          <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--muted)" strokeWidth={stroke} />
          <circle
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            stroke={met ? "var(--success)" : "var(--primary)"}
            strokeWidth={stroke}
            strokeDasharray={c}
            strokeDashoffset={c * (1 - v)}
            strokeLinecap="round"
            className="transition-[stroke-dashoffset] duration-700"
          />
        </svg>
        <div className="absolute inset-0 flex flex-col items-center justify-center">
          <span className="text-2xl font-semibold tabular-nums">{Math.floor(minutes)}</span>
          <span className="text-[11px] text-muted-foreground">of {goal} min</span>
        </div>
      </div>
      <div className="space-y-1">
        <p className="flex items-center gap-1.5 text-lg font-semibold">
          <Flame
            className={streak.current > 0 ? "size-5 text-warning" : "size-5 text-muted-foreground"}
            aria-hidden
          />
          {streak.current}-day streak
        </p>
        <p className="text-xs text-muted-foreground">
          {met ? "Goal met today 🎉" : `${Math.max(0, Math.ceil(goal - minutes))} min to go today`} · best{" "}
          {streak.longest}
        </p>
        <p
          className="flex items-center gap-1 text-xs text-muted-foreground"
          title="One missed day per week is forgiven automatically"
        >
          <Snowflake className="size-3.5" aria-hidden />
          {streak.freezeUsedThisWeek ? "Freeze used this week" : "Freeze available this week"}
        </p>
      </div>
    </div>
  );
}
