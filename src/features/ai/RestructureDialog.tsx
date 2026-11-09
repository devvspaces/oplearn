import { useEffect, useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ArrowDown, ArrowUp, Sparkles } from "lucide-react";
import type { CourseDetail, RestructureProposal, StructureModule } from "@/bindings";
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
import { Input } from "@/components/ui/input";
import { Badge, Spinner } from "@/components/ui/misc";
import { formatDuration } from "@/lib/time";
import { useAiStatus } from "./AiSettings";

function money(usd: number | null | undefined) {
  const v = usd ?? 0;
  return v < 0.01 ? "< $0.01" : `$${v.toFixed(2)}`;
}

/** Moves a lesson to another module (pure; exported for tests). */
export function moveLessonTo(mods: StructureModule[], lessonId: string, target: number): StructureModule[] {
  const next = mods.map((m) => ({ ...m, lessonIds: m.lessonIds.filter((id) => id !== lessonId) }));
  next[target] = { ...next[target]!, lessonIds: [...next[target]!.lessonIds, lessonId] };
  return next.filter((m) => m.lessonIds.length > 0);
}

export function RestructureDialog({
  detail,
  open,
  onOpenChange,
}: {
  detail: CourseDetail;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const qc = useQueryClient();
  const courseId = detail.course.id;
  const { data: ai } = useAiStatus();
  const [withTranscripts, setWithTranscripts] = useState(false);
  const [busy, setBusy] = useState(false);
  const [proposal, setProposal] = useState<RestructureProposal | null>(null);
  const [mods, setMods] = useState<StructureModule[]>([]);
  useEffect(() => {
    if (!open) {
      setProposal(null);
      setMods([]);
    }
  }, [open]);
  const ready = !!ai?.settings.enabled && ai.hasKey;
  const estimate = useQuery({
    queryKey: ["restructureEstimate", courseId, withTranscripts, ai?.settings.model],
    queryFn: () => commands.estimateRestructure(courseId, withTranscripts),
    enabled: open && ready && !proposal,
  });
  const lessons = useMemo(
    () => new Map(detail.modules.flatMap((m) => m.lessons).map((l) => [l.id, l])),
    [detail],
  );

  async function generate() {
    setBusy(true);
    try {
      const p = await commands.proposeRestructure(courseId, withTranscripts);
      setProposal(p);
      setMods(p.modules);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    setBusy(true);
    try {
      await commands.applyCourseStructure(courseId, mods);
      for (const k of [["course"], ["courses"], ["lesson"], ["canUndoRestructure"]])
        void qc.invalidateQueries({ queryKey: k });
      onOpenChange(false);
      toast.success(`Course reorganised into ${mods.length} modules`, {
        duration: 10_000,
        action: {
          label: "Undo",
          onClick: () =>
            void commands.undoRestructure(courseId).then(
              () => {
                for (const k of [["course"], ["courses"], ["canUndoRestructure"]])
                  void qc.invalidateQueries({ queryKey: k });
                toast.success("Restored the previous outline");
              },
              (e: unknown) => toast.error(errorMessage(e)),
            ),
        },
      });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const moveModule = (i: number, d: -1 | 1) =>
    setMods((m) => {
      const n = m.slice();
      const j = i + d;
      if (j < 0 || j >= n.length) return m;
      [n[i], n[j]] = [n[j]!, n[i]!];
      return n;
    });

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent className="max-h-[90vh] max-w-4xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-5 text-primary" /> Restructure with AI
          </DialogTitle>
          <DialogDescription>
            Claude proposes modules and an order for this course's lessons. You can review and edit the
            proposal before anything changes, and undo it afterwards.
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 overflow-y-auto">
          {!ready ? (
            <p className="rounded-lg border bg-muted/50 p-4 text-sm">
              Turn on AI features and add your Anthropic API key in{" "}
              <Link
                to="/settings"
                className="text-primary hover:underline"
                onClick={() => onOpenChange(false)}
              >
                Settings → AI
              </Link>{" "}
              first.
            </p>
          ) : !proposal ? (
            <div className="space-y-4 text-sm">
              <label className="flex items-start gap-2">
                <input
                  type="checkbox"
                  className="mt-1"
                  checked={withTranscripts}
                  onChange={(e) => setWithTranscripts(e.target.checked)}
                />
                <span>
                  Include the first two minutes of each transcript
                  <span className="block text-xs text-muted-foreground">
                    Better grouping when titles are vague (e.g. "Lecture 7"). Only fetched transcripts are
                    used.
                  </span>
                </span>
              </label>
              <div className="rounded-lg border p-3 text-xs">
                <p className="mb-1 font-medium text-foreground">What will be sent to Anthropic</p>
                <p className="text-muted-foreground">
                  The course title and description, plus each lesson's title, length, description excerpt and
                  chapter titles
                  {withTranscripts ? ", and transcript openings" : ""}. Your notes are not sent.
                </p>
                <p className="mt-2 tabular-nums">
                  {estimate.data ? (
                    <>
                      {estimate.data.items} lessons · ~
                      {(estimate.data.inputTokens + estimate.data.outputTokens).toLocaleString()} tokens ·
                      estimated <b>{money(estimate.data.costUsd)}</b> with {estimate.data.model}
                    </>
                  ) : (
                    <Spinner />
                  )}
                </p>
              </div>
            </div>
          ) : (
            <div className="space-y-4">
              <div className="rounded-lg border bg-muted/40 p-3 text-xs">
                <p className="flex flex-wrap items-center gap-2">
                  <Badge tone="primary">AI-generated</Badge>
                  <span className="text-muted-foreground">
                    {proposal.model} · {money(proposal.costUsd)} ·{" "}
                    {(proposal.inputTokens + proposal.outputTokens).toLocaleString()} tokens
                  </span>
                </p>
                <p className="mt-2 text-sm">{proposal.rationale}</p>
                {(proposal.unsorted > 0 || proposal.dropped > 0) && (
                  <p className="mt-1 text-warning">
                    {proposal.unsorted > 0 &&
                      `${proposal.unsorted} lesson(s) weren't placed and are in "Unsorted". `}
                    {proposal.dropped > 0 && `${proposal.dropped} unknown or duplicate id(s) were ignored.`}
                  </p>
                )}
              </div>
              <div className="grid gap-4 md:grid-cols-[1fr_1.6fr]">
                <section aria-label="Current outline">
                  <h3 className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                    Current
                  </h3>
                  <ol className="space-y-2 text-xs">
                    {detail.modules.map((m) => (
                      <li key={m.module.id} className="rounded-lg border p-2">
                        <p className="font-medium">{m.module.title}</p>
                        <p className="text-muted-foreground">{m.lessons.length} lessons</p>
                      </li>
                    ))}
                  </ol>
                </section>
                <section aria-label="Proposed outline">
                  <h3 className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                    Proposed (editable)
                  </h3>
                  <ol className="space-y-3">
                    {mods.map((m, mi) => (
                      <li key={mi} className="rounded-lg border bg-card p-2">
                        <div className="flex items-center gap-1">
                          <Input
                            value={m.title}
                            aria-label={`Module ${mi + 1} title`}
                            onChange={(e) =>
                              setMods((all) =>
                                all.map((x, j) => (j === mi ? { ...x, title: e.target.value } : x)),
                              )
                            }
                            className="h-7 text-sm font-medium"
                          />
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-label="Move module up"
                            disabled={mi === 0}
                            onClick={() => moveModule(mi, -1)}
                          >
                            <ArrowUp />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-label="Move module down"
                            disabled={mi === mods.length - 1}
                            onClick={() => moveModule(mi, 1)}
                          >
                            <ArrowDown />
                          </Button>
                        </div>
                        {m.summary && (
                          <p className="mt-1 px-1 text-[11px] text-muted-foreground">{m.summary}</p>
                        )}
                        <ul className="mt-1 space-y-0.5">
                          {m.lessonIds.map((id) => {
                            const l = lessons.get(id);
                            return (
                              <li
                                key={id}
                                className="flex items-center gap-2 rounded px-1 py-0.5 text-xs hover:bg-accent/50"
                              >
                                <span className="min-w-0 flex-1 truncate">{l?.title ?? id}</span>
                                <span className="shrink-0 tabular-nums text-muted-foreground">
                                  {formatDuration(l?.durationSec)}
                                </span>
                                <select
                                  aria-label={`Move ${l?.title ?? "lesson"} to module`}
                                  value={mi}
                                  onChange={(e) =>
                                    setMods((all) => moveLessonTo(all, id, Number(e.target.value)))
                                  }
                                  className="max-w-28 shrink-0 rounded border bg-background px-1 py-0.5 text-[10px]"
                                >
                                  {mods.map((x, j) => (
                                    <option key={j} value={j}>
                                      {j + 1}. {x.title.slice(0, 24)}
                                    </option>
                                  ))}
                                </select>
                              </li>
                            );
                          })}
                        </ul>
                      </li>
                    ))}
                  </ol>
                </section>
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          {proposal ? (
            <>
              <Button variant="outline" onClick={() => void generate()} disabled={busy}>
                Try again
              </Button>
              <Button onClick={() => void apply()} disabled={busy || mods.some((m) => !m.title.trim())}>
                {busy ? "Applying…" : "Apply new outline"}
              </Button>
            </>
          ) : (
            <Button onClick={() => void generate()} disabled={!ready || busy || !estimate.data}>
              {busy ? (
                <>
                  <Spinner /> Asking Claude…
                </>
              ) : (
                <>
                  <Sparkles /> Generate proposal {estimate.data ? `(~${money(estimate.data.costUsd)})` : ""}
                </>
              )}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
