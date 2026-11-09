import { Bell, Plus, X } from "lucide-react";
import { toast } from "sonner";
import type { StudySettings } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { useSaveStudySettings, useStudySettings } from "./api";

const DAYS: Array<[string, string]> = [
  ["mon", "M"],
  ["tue", "T"],
  ["wed", "W"],
  ["thu", "T"],
  ["fri", "F"],
  ["sat", "S"],
  ["sun", "S"],
];
const DAY_NAMES: Record<string, string> = {
  mon: "Monday",
  tue: "Tuesday",
  wed: "Wednesday",
  thu: "Thursday",
  fri: "Friday",
  sat: "Saturday",
  sun: "Sunday",
};

export function StudySettingsForm() {
  const { data: s } = useStudySettings();
  const save = useSaveStudySettings();
  if (!s) return null;
  const studyDays = s.studyDays ?? [];
  const reminders = s.reminders ?? [];
  const patch = (p: Partial<StudySettings>) => save.mutate({ ...s, ...p });
  const threshold = Math.round((s.completionThreshold ?? 0.9) * 100);

  return (
    <div className="space-y-5 text-sm">
      <div className="flex flex-wrap items-center gap-6">
        <label className="flex items-center gap-2">
          Daily goal
          <Input
            type="number"
            min={5}
            max={600}
            step={5}
            defaultValue={s.dailyGoalMin}
            key={`goal-${s.dailyGoalMin}`}
            onBlur={(e) =>
              Number(e.target.value) !== s.dailyGoalMin &&
              patch({ dailyGoalMin: Number(e.target.value) || 45 })
            }
            className="h-8 w-20"
          />
          min
        </label>
        <label className="flex items-center gap-2">
          Auto-complete a video at
          <select
            value={threshold}
            onChange={(e) => patch({ completionThreshold: Number(e.target.value) / 100 })}
            className="rounded-md border bg-background px-2 py-1 text-xs"
          >
            {[70, 75, 80, 85, 90, 95, 100].map((n) => (
              <option key={n} value={n}>
                {n}% watched
              </option>
            ))}
          </select>
        </label>
      </div>

      <div>
        <p className="mb-1.5">Study days</p>
        <div className="flex gap-1" role="group" aria-label="Study days">
          {DAYS.map(([k, label]) => {
            const on = studyDays.includes(k);
            return (
              <button
                key={k}
                type="button"
                aria-pressed={on}
                aria-label={DAY_NAMES[k]}
                title={DAY_NAMES[k]}
                onClick={() =>
                  patch({ studyDays: on ? studyDays.filter((d) => d !== k) : [...studyDays, k] })
                }
                className={cn(
                  "size-8 rounded-full border text-xs font-medium",
                  on
                    ? "border-primary bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-accent",
                )}
              >
                {label}
              </button>
            );
          })}
        </div>
        <p className="mt-1 text-xs text-muted-foreground">
          Plans only schedule lessons on these days. Rebalance a course after changing them.
        </p>
      </div>

      <div>
        <p className="mb-1.5 flex items-center gap-1.5">
          <Bell className="size-4" /> Reminders
        </p>
        <ul className="space-y-1.5">
          {reminders.map((r, i) => (
            <li key={i} className="flex items-center gap-2">
              <input
                type="checkbox"
                aria-label={`Reminder at ${r.time} enabled`}
                checked={r.enabled}
                onChange={(e) =>
                  patch({
                    reminders: reminders.map((x, j) => (j === i ? { ...x, enabled: e.target.checked } : x)),
                  })
                }
              />
              <Input
                type="time"
                value={r.time}
                onChange={(e) =>
                  patch({
                    reminders: reminders.map((x, j) => (j === i ? { ...x, time: e.target.value } : x)),
                  })
                }
                className="h-8 w-28"
              />
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Remove reminder"
                onClick={() => patch({ reminders: reminders.filter((_, j) => j !== i) })}
              >
                <X />
              </Button>
            </li>
          ))}
        </ul>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => patch({ reminders: [...reminders, { time: "08:00", enabled: true }] })}
          >
            <Plus /> Add reminder
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => commands.testReminder().catch((e: unknown) => toast.error(errorMessage(e)))}
          >
            Send a test notification
          </Button>
        </div>
        <label className="mt-3 flex items-center gap-2">
          <input
            type="checkbox"
            checked={s.behindNudge}
            onChange={(e) => patch({ behindNudge: e.target.checked })}
          />
          Nudge me in the evening when a course falls behind its plan
        </label>
        <p className="mt-1 text-xs text-muted-foreground">
          Reminders are skipped once you've met the day's goal.
        </p>
      </div>
    </div>
  );
}
