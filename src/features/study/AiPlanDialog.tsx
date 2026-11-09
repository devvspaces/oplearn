import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Sparkles } from "lucide-react";
import type { PlanProposal } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input, Label } from "@/components/ui/input";
import { Badge, Spinner } from "@/components/ui/misc";
import { formatDay, todayYmd } from "@/lib/dates";
import { formatDuration } from "@/lib/time";
import { CostLine, money, overBudget } from "@/features/ai/CostLine";
import { useAiStatus } from "@/features/ai/AiSettings";
import { sk } from "./api";

/** AI weekly study plan using the course's syllabus, calendar and materials when available. */
export function AiPlanDialog({
  courseId,
  open,
  onOpenChange,
}: {
  courseId: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const qc = useQueryClient();
  const { data: ai } = useAiStatus();
  const ready = !!ai?.settings.enabled && ai.hasKey;
  const [date, setDate] = useState("");
  const [proposal, setProposal] = useState<PlanProposal | null>(null);
  const [busy, setBusy] = useState(false);
  const target = date || null;
  const estimate = useQuery({
    queryKey: ["planEstimate", courseId, target, ai?.settings.model],
    queryFn: () => commands.estimateCoursePlan(courseId, target),
    enabled: open && ready && !proposal,
    retry: false,
  });
  const close = (o: boolean) => {
    if (busy) return;
    onOpenChange(o);
    if (!o) setProposal(null);
  };

  async function generate() {
    setBusy(true);
    try {
      setProposal(await commands.proposeCoursePlan(courseId, target));
      void qc.invalidateQueries({ queryKey: ["aiUsage"] });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  async function apply() {
    if (!proposal) return;
    setBusy(true);
    try {
      const p = await commands.applyCoursePlan(
        courseId,
        proposal.weeks.flatMap((w) => w.items.map((i) => ({ lessonId: i.lessonId, date: i.date }))),
      );
      qc.setQueryData(sk.plan(courseId), p);
      void qc.invalidateQueries({ queryKey: sk.all });
      toast.success("Study plan applied");
      onOpenChange(false);
      setProposal(null);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="max-h-[90vh] max-w-2xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-5 text-primary" /> Plan with AI
          </DialogTitle>
          <DialogDescription>
            Claude orders the remaining lessons into weeks, putting readings beside their lectures and problem
            sets after them, within your daily goal. Uses the syllabus and calendar for imported courses.
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 overflow-y-auto">
          {!ready ? (
            <p className="rounded-lg border bg-muted/50 p-4 text-sm">
              Turn on AI and add your Anthropic API key in{" "}
              <Link
                to="/settings"
                className="text-primary hover:underline"
                onClick={() => onOpenChange(false)}
              >
                Settings → AI
              </Link>
              .
            </p>
          ) : !proposal ? (
            <div className="space-y-4 text-sm">
              <div className="grid max-w-xs gap-1">
                <Label htmlFor="ai-plan-date">Finish by (optional)</Label>
                <Input
                  id="ai-plan-date"
                  type="date"
                  min={todayYmd()}
                  value={date}
                  onChange={(e) => setDate(e.target.value)}
                />
              </div>
              <div className="rounded-lg border p-3">
                {estimate.isError ? (
                  <p className="text-xs text-destructive">{errorMessage(estimate.error)}</p>
                ) : (
                  <CostLine
                    estimate={estimate.data}
                    what={estimate.data ? `${estimate.data.items} lessons` : undefined}
                  />
                )}
              </div>
            </div>
          ) : (
            <div className="space-y-4">
              <p className="rounded-lg border bg-muted/40 p-3 text-xs">
                <Badge tone="primary" className="mr-2">
                  AI-generated
                </Badge>
                {proposal.rationale}
                <span className="ml-1 text-muted-foreground">({money(proposal.costUsd)})</span>
              </p>
              <ol className="space-y-3">
                {proposal.weeks.map((w, i) => (
                  <li key={i} className="rounded-lg border p-3">
                    <p className="text-sm font-medium">
                      Week {i + 1}
                      {w.items[0] ? ` · from ${formatDay(w.items[0].date)}` : ""}
                    </p>
                    <p className="text-xs text-muted-foreground">{w.focus}</p>
                    <ul className="mt-2 space-y-0.5 text-xs">
                      {w.items.map((it) => (
                        <li key={it.lessonId} className="flex gap-2">
                          <span className="w-20 shrink-0 text-muted-foreground">
                            {formatDay(it.date, { weekday: "short", day: "numeric" })}
                          </span>
                          <span className="min-w-0 flex-1 truncate">{it.title}</span>
                          <span className="tabular-nums text-muted-foreground">
                            {formatDuration(it.durationSec)}
                          </span>
                        </li>
                      ))}
                    </ul>
                  </li>
                ))}
              </ol>
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => close(false)} disabled={busy}>
            Cancel
          </Button>
          {proposal ? (
            <>
              <Button variant="outline" onClick={() => void generate()} disabled={busy}>
                Try again
              </Button>
              <Button onClick={() => void apply()} disabled={busy}>
                Apply plan
              </Button>
            </>
          ) : (
            <Button
              onClick={() => void generate()}
              disabled={!ready || busy || !estimate.data || overBudget(estimate.data)}
            >
              {busy ? (
                <>
                  <Spinner /> Planning…
                </>
              ) : (
                <>
                  <Sparkles /> Generate plan {estimate.data ? `(~${money(estimate.data.costUsd)})` : ""}
                </>
              )}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
