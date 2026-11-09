import { useMemo, useReducer, useRef, useState } from "react";
import { Link } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  CheckCircle2,
  FileText,
  Layers,
  ListChecks,
  PlayCircle,
  Sparkles,
  Trash2,
  XCircle,
} from "lucide-react";
import type { QuizDraft, QuizHint, QuizQuestion, QuizScope } from "@/bindings";
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
import { Textarea } from "@/components/ui/input";
import { Badge, EmptyState, Spinner } from "@/components/ui/misc";
import { formatTimestamp } from "@/lib/time";
import { cn } from "@/lib/utils";
import { CostLine, money, overBudget } from "@/features/ai/CostLine";
import { useAiStatus } from "@/features/ai/AiSettings";
import { useCardDialog } from "@/features/flashcards/api";
import { isFinished, runReducer, score, shuffled, startRun } from "./runner";

const qkQuiz = (scope: QuizScope, id: string) => ["quiz", scope, id] as const;

/** Where a hint takes the learner. */
export function HintLink({ hint, onNavigate }: { hint: QuizHint; onNavigate?: () => void }) {
  if (hint.kind === "video")
    return (
      <Link
        to="/lessons/$lessonId"
        params={{ lessonId: hint.lessonId }}
        search={{ t: Math.floor(hint.sec ?? 0) }}
        onClick={onNavigate}
        className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
      >
        <PlayCircle className="size-3.5" /> Watch at {formatTimestamp(hint.sec ?? 0)}
      </Link>
    );
  if (hint.kind === "pdf")
    return (
      <Link
        to="/materials/$materialId"
        params={{ materialId: hint.materialId }}
        search={{ page: hint.page }}
        onClick={onNavigate}
        className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
      >
        <FileText className="size-3.5" /> Lecture notes p.{hint.page}
      </Link>
    );
  return null;
}

export function QuizPanel({
  scope,
  scopeId,
  courseId,
  compact,
}: {
  scope: QuizScope;
  scopeId: string;
  courseId: string;
  compact?: boolean;
}) {
  const { data: questions, isLoading } = useQuery({
    queryKey: qkQuiz(scope, scopeId),
    queryFn: () => commands.listQuiz(scope, scopeId),
  });
  const [mode, setMode] = useState<"bank" | "run">("bank");
  const [genOpen, setGenOpen] = useState(false);
  const [onlyWrong, setOnlyWrong] = useState(false);
  const qc = useQueryClient();
  const del = useMutation({
    mutationFn: commands.deleteQuizQuestion,
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["quiz"] }),
    onError: (e) => toast.error(errorMessage(e)),
  });
  const pool = useMemo(
    () => (questions ?? []).filter((q) => !onlyWrong || q.lastCorrect === false),
    [questions, onlyWrong],
  );
  const total = questions?.length ?? 0;
  const tried = (questions ?? []).filter((q) => q.attempts > 0);
  const lastRight = tried.filter((q) => q.lastCorrect).length;

  if (isLoading) return <Spinner />;
  if (mode === "run" && pool.length)
    return <Runner questions={pool} onExit={() => setMode("bank")} compact={compact} />;

  return (
    <div className={cn("flex min-h-0 flex-col", compact ? "h-full" : "")}>
      <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2">
        <span className="text-xs text-muted-foreground">
          {total} question{total === 1 ? "" : "s"}
          {tried.length ? ` · ${lastRight}/${tried.length} right last time` : ""}
        </span>
        <div className="ml-auto flex gap-1.5">
          <Button size="sm" variant="outline" onClick={() => setGenOpen(true)}>
            <Sparkles /> Generate with AI
          </Button>
          <Button size="sm" disabled={!pool.length} onClick={() => setMode("run")}>
            <ListChecks /> Start quiz
          </Button>
        </div>
      </div>
      {tried.some((q) => q.lastCorrect === false) && (
        <label className="flex items-center gap-2 px-3 pt-2 text-xs">
          <input type="checkbox" checked={onlyWrong} onChange={(e) => setOnlyWrong(e.target.checked)} />
          Only questions I got wrong last time
        </label>
      )}
      <div className={cn("min-h-0 flex-1 overflow-y-auto p-2", !compact && "max-h-[60vh]")}>
        {!total ? (
          <EmptyState icon={<ListChecks className="size-8" />} title="No quiz questions yet">
            Generate a question bank from the transcript, your notes and lecture notes. Every question links
            to where the answer is taught.
          </EmptyState>
        ) : (
          <ul className="space-y-1.5">
            {questions!.map((q) => (
              <li
                key={q.id}
                className="group flex items-start gap-2 rounded-lg border bg-background/50 p-2 text-sm"
              >
                <span className="mt-0.5 shrink-0">
                  {q.lastCorrect == null ? (
                    <Badge tone="muted">{q.kind === "mcq" ? "MCQ" : "Short"}</Badge>
                  ) : q.lastCorrect ? (
                    <CheckCircle2 className="size-4 text-success" aria-label="Right last time" />
                  ) : (
                    <XCircle className="size-4 text-warning" aria-label="Wrong last time" />
                  )}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="line-clamp-2">{q.promptMd}</p>
                  <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
                    {q.difficulty && <span className="capitalize">{q.difficulty}</span>}
                    {q.lessonTitle && scope !== "lesson" && <span className="truncate">{q.lessonTitle}</span>}
                    <HintLink hint={q.hint} />
                    {q.aiGenerated && <Badge tone="primary">AI</Badge>}
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Delete question"
                  className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                  onClick={() => del.mutate(q.id)}
                >
                  <Trash2 />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <GenerateQuizDialog
        open={genOpen}
        onOpenChange={setGenOpen}
        scope={scope}
        scopeId={scopeId}
        courseId={courseId}
      />
    </div>
  );
}

function Runner({
  questions,
  onExit,
  compact,
}: {
  questions: QuizQuestion[];
  onExit: () => void;
  compact?: boolean;
}) {
  const qc = useQueryClient();
  const ids = useMemo(() => questions.map((q) => q.id), [questions]);
  const byId = useMemo(() => new Map(questions.map((q) => [q.id, q])), [questions]);
  const [state, dispatch] = useReducer(runReducer, ids, (x) => startRun(x, shuffled));
  const [picked, setPicked] = useState<number | null>(null);
  const [revealed, setRevealed] = useState(false);
  const [own, setOwn] = useState("");
  const shownAt = useRef(Date.now());
  const q = byId.get(state.order[state.index] ?? "");
  const result = q ? state.results[q.id] : undefined;

  const answer = (correct: boolean) => {
    if (!q || result !== undefined) return;
    dispatch({ type: "answer", id: q.id, correct });
    const seconds = Math.min(180, (Date.now() - shownAt.current) / 1000);
    void commands.recordQuizAnswer(q.id, correct, seconds).then(
      () => void qc.invalidateQueries({ queryKey: ["quiz"] }),
      () => undefined,
    );
  };
  const next = () => {
    dispatch({ type: "next" });
    setPicked(null);
    setRevealed(false);
    setOwn("");
    shownAt.current = Date.now();
  };
  const makeCard = () => {
    if (!q) return;
    const lessonId = q.hint.kind === "video" ? q.hint.lessonId : q.lessonId;
    useCardDialog.getState().open({
      lessonId,
      sourceSec: q.hint.kind === "video" ? q.hint.sec : null,
      sourceMaterialId: q.hint.kind === "pdf" ? q.hint.materialId : null,
      sourcePage: q.hint.kind === "pdf" ? q.hint.page : null,
      front: q.promptMd,
      back: [q.answerMd, q.explanationMd].filter(Boolean).join("\n\n"),
    });
  };

  if (isFinished(state)) {
    const s = score(state);
    const wrong = s.answered - s.correct;
    return (
      <div className="flex flex-col items-center gap-3 p-8 text-center">
        <p className="text-3xl font-semibold tabular-nums">
          {s.correct}/{s.total}
        </p>
        <p className="text-sm text-muted-foreground">
          {s.correct === s.total
            ? "Perfect score."
            : `${wrong} to revisit. Use the hints to jump to where they're taught.`}
        </p>
        <div className="flex gap-2">
          {wrong > 0 && (
            <Button onClick={() => dispatch({ type: "restart", ids, wrongOnly: true, shuffle: shuffled })}>
              Retry wrong ones
            </Button>
          )}
          <Button variant="outline" onClick={() => dispatch({ type: "restart", ids, shuffle: shuffled })}>
            Start again
          </Button>
          <Button variant="ghost" onClick={onExit}>
            Done
          </Button>
        </div>
      </div>
    );
  }
  if (!q) return null;
  const answered = result !== undefined;
  return (
    <div className={cn("flex min-h-0 flex-col gap-3 overflow-y-auto p-3", compact ? "h-full" : "")}>
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>
          Question {state.index + 1} of {state.order.length}
          {q.difficulty ? ` · ${q.difficulty}` : ""}
        </span>
        <button className="hover:text-foreground" onClick={onExit}>
          Exit
        </button>
      </div>
      <p className="whitespace-pre-wrap font-medium leading-relaxed">{q.promptMd}</p>
      {q.kind === "mcq" ? (
        <div className="grid gap-1.5" role="radiogroup" aria-label="Choices">
          {q.choices.map((c, i) => {
            const isAnswer = i === q.answerIndex;
            return (
              <button
                key={i}
                role="radio"
                aria-checked={picked === i}
                disabled={answered}
                onClick={() => {
                  setPicked(i);
                  answer(i === q.answerIndex);
                }}
                className={cn(
                  "rounded-lg border p-2.5 text-left text-sm transition-colors",
                  !answered && "hover:bg-accent",
                  answered && isAnswer && "border-success bg-success/10",
                  answered && picked === i && !isAnswer && "border-destructive bg-destructive/10",
                )}
              >
                <span className="mr-2 text-xs text-muted-foreground">{String.fromCharCode(65 + i)}.</span>
                {c}
              </button>
            );
          })}
        </div>
      ) : (
        <div className="grid gap-2">
          <Textarea
            rows={3}
            value={own}
            onChange={(e) => setOwn(e.target.value)}
            placeholder="Your answer (optional; for your own reference)"
            disabled={revealed}
          />
          {!revealed ? (
            <Button variant="outline" onClick={() => setRevealed(true)}>
              Reveal model answer
            </Button>
          ) : (
            <div className="rounded-lg border bg-muted/40 p-3 text-sm">
              <p className="mb-1 text-xs font-medium text-muted-foreground">Model answer</p>
              <p className="whitespace-pre-wrap">{q.answerMd}</p>
              {!answered && (
                <div className="mt-3 flex gap-2">
                  <Button size="sm" onClick={() => answer(true)}>
                    <CheckCircle2 /> I got it right
                  </Button>
                  <Button size="sm" variant="outline" onClick={() => answer(false)}>
                    <XCircle /> I got it wrong
                  </Button>
                </div>
              )}
            </div>
          )}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-3">
        {!answered && q.hint.kind !== "none" && (
          <span className="text-xs text-muted-foreground">
            Need a hint? <HintLink hint={q.hint} />
          </span>
        )}
      </div>
      {answered && (
        <div
          className={cn("rounded-lg border p-3 text-sm", result ? "border-success/40" : "border-warning/40")}
        >
          <p className="font-medium">{result ? "Correct" : "Not quite"}</p>
          {q.explanationMd && (
            <p className="mt-1 whitespace-pre-wrap text-muted-foreground">{q.explanationMd}</p>
          )}
          <div className="mt-2 flex flex-wrap items-center gap-3">
            <HintLink hint={q.hint} />
            {!result && (
              <Button size="sm" variant="ghost" onClick={makeCard}>
                <Layers /> Make a flashcard
              </Button>
            )}
          </div>
        </div>
      )}
      {answered && (
        <Button className="self-end" onClick={next}>
          {state.index + 1 < state.order.length ? "Next question" : "See results"}
        </Button>
      )}
    </div>
  );
}

const MIXES: Array<[number, string]> = [
  [1, "All multiple choice"],
  [0.7, "Mostly multiple choice"],
  [0.5, "Half and half"],
  [0.2, "Mostly short answer"],
];

function GenerateQuizDialog({
  open,
  onOpenChange,
  scope,
  scopeId,
  courseId,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  scope: QuizScope;
  scopeId: string;
  courseId: string;
}) {
  const qc = useQueryClient();
  const { data: ai } = useAiStatus();
  const ready = !!ai?.settings.enabled && ai.hasKey;
  const [count, setCount] = useState(10);
  const [mix, setMix] = useState(0.7);
  const [drafts, setDrafts] = useState<QuizDraft[] | null>(null);
  const [keep, setKeep] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const estimate = useQuery({
    queryKey: ["quizEstimate", scope, scopeId, count, mix, ai?.settings.model],
    queryFn: () => commands.estimateQuiz(scope, scopeId, count, mix),
    enabled: open && ready && !drafts,
    retry: false,
  });
  const close = (o: boolean) => {
    if (busy) return;
    onOpenChange(o);
    if (!o) setDrafts(null);
  };

  async function generate() {
    setBusy(true);
    try {
      const r = await commands.generateQuiz(scope, scopeId, count, mix);
      setDrafts(r.drafts);
      setKeep(new Set(r.drafts.map((_, i) => i)));
      if (r.dropped) toast.message(`${r.dropped} malformed question(s) were discarded`);
      void qc.invalidateQueries({ queryKey: ["aiUsage"] });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (!drafts) return;
    setBusy(true);
    try {
      const n = await commands.saveQuizQuestions(
        courseId,
        drafts.filter((_, i) => keep.has(i)),
        true,
      );
      toast.success(`Saved ${n} question${n === 1 ? "" : "s"}`);
      void qc.invalidateQueries({ queryKey: ["quiz"] });
      onOpenChange(false);
      setDrafts(null);
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
            <Sparkles className="size-5 text-primary" /> Generate a quiz
          </DialogTitle>
          <DialogDescription>
            Built from {scope === "lesson" ? "this lesson's" : "the course's"} transcripts, your notes and
            lecture notes. Review the questions before saving.
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
          ) : !drafts ? (
            <div className="space-y-4 text-sm">
              <div className="flex flex-wrap gap-6">
                <label className="flex items-center gap-2">
                  Questions
                  <select
                    value={count}
                    onChange={(e) => setCount(Number(e.target.value))}
                    className="rounded-md border bg-background px-2 py-1 text-xs"
                  >
                    {[5, 10, 15, 20, 30].map((n) => (
                      <option key={n}>{n}</option>
                    ))}
                  </select>
                </label>
                <label className="flex items-center gap-2">
                  Mix
                  <select
                    value={mix}
                    onChange={(e) => setMix(Number(e.target.value))}
                    className="rounded-md border bg-background px-2 py-1 text-xs"
                  >
                    {MIXES.map(([v, l]) => (
                      <option key={v} value={v}>
                        {l}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <div className="rounded-lg border p-3">
                {estimate.isError ? (
                  <p className="text-xs text-destructive">{errorMessage(estimate.error)}</p>
                ) : (
                  <CostLine
                    estimate={estimate.data}
                    what={estimate.data ? `${estimate.data.items} questions` : undefined}
                  />
                )}
              </div>
            </div>
          ) : (
            <ul className="space-y-2">
              {drafts.map((d, i) => (
                <li key={i} className="flex gap-2 rounded-lg border p-2 text-sm">
                  <input
                    type="checkbox"
                    className="mt-1"
                    aria-label={`Keep question ${i + 1}`}
                    checked={keep.has(i)}
                    onChange={(e) =>
                      setKeep((k) => {
                        const n = new Set(k);
                        if (e.target.checked) n.add(i);
                        else n.delete(i);
                        return n;
                      })
                    }
                  />
                  <div className="min-w-0 flex-1">
                    <p className="font-medium">{d.promptMd}</p>
                    {d.kind === "mcq" ? (
                      <ol className="mt-1 list-[upper-alpha] pl-5 text-xs text-muted-foreground">
                        {d.choices.map((c, j) => (
                          <li key={j} className={j === d.answerIndex ? "font-medium text-success" : ""}>
                            {c}
                          </li>
                        ))}
                      </ol>
                    ) : (
                      <p className="mt-1 text-xs text-muted-foreground">Answer: {d.answerMd}</p>
                    )}
                    <div className="mt-1 flex gap-2">
                      <Badge tone="muted">{d.kind === "mcq" ? "MCQ" : "Short"}</Badge>
                      <HintLink hint={d.hint} onNavigate={() => onOpenChange(false)} />
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => close(false)} disabled={busy}>
            Cancel
          </Button>
          {drafts ? (
            <Button onClick={() => void save()} disabled={busy || keep.size === 0}>
              Save {keep.size} question{keep.size === 1 ? "" : "s"}
            </Button>
          ) : (
            <Button
              onClick={() => void generate()}
              disabled={!ready || busy || !estimate.data || overBudget(estimate.data)}
            >
              {busy ? (
                <>
                  <Spinner /> Writing questions…
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
