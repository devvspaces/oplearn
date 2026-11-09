import { useEffect, useState } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Sparkles } from "lucide-react";
import type { AssistAnswer, AssistFocus, AssistKind, SourceRef } from "@/bindings";
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
import { Label, Textarea } from "@/components/ui/input";
import { Badge, Spinner } from "@/components/ui/misc";
import { player } from "@/features/player/store";
import { pdfNav } from "@/features/materials/pdf-nav";
import { CostLine, money, overBudget } from "./CostLine";
import { useAiStatus } from "./AiSettings";

const TITLES: Record<AssistKind, string> = {
  explain: "Explain with AI",
  answer: "Answer with AI",
  summary: "Summarize with AI",
};

/** Jumps the open player or PDF to a cited source; falls back to a link elsewhere. */
export function SourceChip({ s, lessonId }: { s: SourceRef; lessonId: string }) {
  const here = (s.lessonId ?? lessonId) === lessonId;
  const cls = "rounded border px-1.5 py-0.5 font-mono text-[11px] text-primary hover:bg-primary/10";
  if (here && s.sec != null)
    return (
      <button className={cls} onClick={() => player.seek(s.sec ?? 0)}>
        {s.label}
      </button>
    );
  if (here && s.materialId && s.page != null && pdfIsOpen(s.materialId))
    return (
      <button className={cls} onClick={() => pdfNav.goTo(s.materialId!, s.page!)}>
        {s.label}
      </button>
    );
  return (
    <Link
      className={cls}
      to="/lessons/$lessonId"
      params={{ lessonId: s.lessonId ?? lessonId }}
      search={s.sec != null ? { t: Math.floor(s.sec) } : { doc: s.materialId ?? undefined, page: s.page ?? undefined }}
    >
      {s.label}
    </Link>
  );
}
const pdfIsOpen = (materialId: string) => pdfNav.current()?.materialId === materialId;

/**
 * Cost preview → generate → review an AI answer, then save it where the caller wants
 * (a question's answer, a new question, the notebook). Nothing is sent without a click.
 */
export function AssistDialog({
  open,
  onOpenChange,
  kind,
  focus,
  question,
  askQuestion,
  description,
  saveLabel,
  onSave,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  kind: AssistKind;
  focus: AssistFocus;
  /** Fixed question (e.g. a question annotation's text). */
  question?: string;
  /** Show an optional "what's unclear?" box (explain). */
  askQuestion?: boolean;
  description: string;
  saveLabel: string;
  onSave: (a: AssistAnswer, question: string) => Promise<unknown>;
}) {
  const qc = useQueryClient();
  const { data: ai } = useAiStatus();
  const ready = !!ai?.settings.enabled && ai.hasKey;
  const [own, setOwn] = useState("");
  const [answer, setAnswer] = useState<AssistAnswer | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) {
      setAnswer(null);
      setOwn("");
    }
  }, [open]);
  const q = (question ?? own).trim() || null;
  const estimate = useQuery({
    queryKey: ["assistEstimate", kind, focus, q, ai?.settings.model],
    queryFn: () => commands.estimateAssist(kind, focus, q),
    enabled: open && ready && !answer,
    retry: false,
  });

  async function generate() {
    setBusy(true);
    try {
      setAnswer(await commands.runAssist(kind, focus, q));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
      void qc.invalidateQueries({ queryKey: ["aiUsage"] });
    }
  }
  async function save() {
    if (!answer) return;
    setBusy(true);
    try {
      await onSave(answer, q ?? "");
      onOpenChange(false);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent className="max-h-[85vh] max-w-xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-5 text-primary" /> {TITLES[kind]}
          </DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto text-sm">
          {!ready ? (
            <p className="rounded-lg border bg-muted/50 p-4">
              Turn on AI and add your Anthropic API key in{" "}
              <Link to="/settings" className="text-primary hover:underline" onClick={() => onOpenChange(false)}>
                Settings → AI
              </Link>
              .
            </p>
          ) : answer ? (
            <>
              <div className="rounded-lg border bg-muted/30 p-3">
                <Badge tone="primary" className="mb-2">
                  AI-generated
                </Badge>
                <div className="whitespace-pre-wrap leading-relaxed">{answer.markdown}</div>
              </div>
              {answer.sources.length > 0 && (
                <div className="flex flex-wrap items-center gap-1.5 text-xs">
                  <span className="text-muted-foreground">Sources:</span>
                  {answer.sources.map((s, i) => (
                    <SourceChip key={i} s={s} lessonId={focus.lessonId} />
                  ))}
                </div>
              )}
              <p className="text-xs text-muted-foreground">Cost {money(answer.costUsd)}</p>
            </>
          ) : (
            <>
              {question && (
                <p className="rounded-lg border bg-muted/30 p-3 text-sm">
                  <span className="text-muted-foreground">Question: </span>
                  {question}
                </p>
              )}
              {askQuestion && (
                <div className="grid gap-1">
                  <Label htmlFor="assist-q">What's unclear? (optional)</Label>
                  <Textarea
                    id="assist-q"
                    rows={2}
                    value={own}
                    onChange={(e) => setOwn(e.target.value)}
                    placeholder="e.g. Why does the inductive step assume P(n)?"
                  />
                </div>
              )}
              <div className="rounded-lg border p-3">
                {estimate.isError ? (
                  <p className="text-xs text-destructive">{errorMessage(estimate.error)}</p>
                ) : (
                  <CostLine estimate={estimate.data} />
                )}
              </div>
            </>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            {answer ? "Discard" : "Cancel"}
          </Button>
          {answer ? (
            <>
              <Button variant="outline" onClick={() => void generate()} disabled={busy}>
                Try again
              </Button>
              <Button onClick={() => void save()} disabled={busy}>
                {saveLabel}
              </Button>
            </>
          ) : (
            <Button
              onClick={() => void generate()}
              disabled={!ready || busy || !estimate.data || overBudget(estimate.data)}
            >
              {busy ? (
                <>
                  <Spinner /> Thinking…
                </>
              ) : (
                <>
                  <Sparkles /> Generate {estimate.data ? `(~${money(estimate.data.costUsd)})` : ""}
                </>
              )}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
