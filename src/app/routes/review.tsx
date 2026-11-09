import { useEffect, useMemo, useRef, useState } from "react";
import { Link, createFileRoute } from "@tanstack/react-router";
import { CheckCircle2, Layers, Pencil, PlayCircle } from "lucide-react";
import type { Flashcard, Grade } from "@/bindings";
import { Button } from "@/components/ui/button";
import { EmptyState, Progress, Spinner } from "@/components/ui/misc";
import { openCardEditor, useDueCards, useReview } from "@/features/flashcards/api";
import { cn } from "@/lib/utils";
import { anchorLabel, anchorSearch, cardAnchor } from "@/lib/anchors";

export const Route = createFileRoute("/review")({ component: Review });

const GRADES: Array<{ grade: Grade; label: string; key: string; hint: string; tone: string }> = [
  {
    grade: "again",
    label: "Again",
    key: "1",
    hint: "Forgot. See it again soon",
    tone: "border-destructive/50 hover:bg-destructive/10",
  },
  {
    grade: "hard",
    label: "Hard",
    key: "2",
    hint: "Recalled with effort",
    tone: "border-warning/50 hover:bg-warning/10",
  },
  { grade: "good", label: "Good", key: "3", hint: "Recalled", tone: "border-primary/50 hover:bg-primary/10" },
  { grade: "easy", label: "Easy", key: "4", hint: "Instant", tone: "border-success/50 hover:bg-success/10" },
];

function Review() {
  const { data, isLoading } = useDueCards();
  const review = useReview();
  // Freeze the session's queue when it loads; "Again" cards go to the back of it.
  const [queue, setQueue] = useState<Flashcard[] | null>(null);
  const [done, setDone] = useState(0);
  const [revealed, setRevealed] = useState(false);
  const shownAt = useRef(Date.now());
  useEffect(() => {
    if (data && queue === null) setQueue(data);
  }, [data, queue]);
  const card = queue?.[0];
  const total = useMemo(() => (queue ? queue.length + done : 0), [queue, done]);

  useEffect(() => {
    shownAt.current = Date.now();
    setRevealed(false);
  }, [card?.id]);

  const grade = (g: Grade) => {
    if (!card || !revealed) return;
    const seconds = Math.min(120, (Date.now() - shownAt.current) / 1000);
    review.mutate({ id: card.id, grade: g, seconds });
    setQueue((q) => {
      if (!q) return q;
      const [first, ...rest] = q;
      return g === "again" && first ? [...rest, first] : rest;
    });
    if (g !== "again") setDone((d) => d + 1);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      if (el.closest("input, textarea, [contenteditable=true]") || e.metaKey || e.ctrlKey) return;
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        setRevealed(true);
      }
      const g = GRADES.find((x) => x.key === e.key);
      if (g) grade(g.grade);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (isLoading || queue === null)
    return (
      <div className="p-8">
        <Spinner />
      </div>
    );

  return (
    <div className="mx-auto max-w-2xl p-8">
      <header className="mb-6 flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Review</h1>
        {total > 0 && (
          <span className="text-sm tabular-nums text-muted-foreground">
            {done}/{total} reviewed
          </span>
        )}
      </header>
      {total > 0 && <Progress value={(100 * done) / total} label="Review progress" className="mb-6" />}

      {!card ? (
        <EmptyState
          icon={done ? <CheckCircle2 className="size-8 text-success" /> : <Layers className="size-8" />}
          title={done ? `All done: ${done} card${done === 1 ? "" : "s"} reviewed` : "No cards due"}
        >
          {done
            ? "Come back tomorrow for the next ones."
            : "Make cards from a note's menu, by selecting transcript text, or in a lesson's Cards tab."}
        </EmptyState>
      ) : (
        <div className="space-y-4">
          <article className="rounded-2xl border bg-card p-8 shadow-sm">
            <div className="mb-4 flex items-center justify-between gap-2 text-xs text-muted-foreground">
              <span className="truncate">{card.courseTitle ?? "No course"}</span>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Edit card"
                onClick={() => openCardEditor(card)}
              >
                <Pencil />
              </Button>
            </div>
            <p className="whitespace-pre-wrap text-lg font-medium leading-relaxed">{card.frontMd}</p>
            {revealed ? (
              <>
                <hr className="my-6" />
                <p className="whitespace-pre-wrap leading-relaxed">
                  {card.backMd || <em className="text-muted-foreground">No answer written</em>}
                </p>
                {card.lessonId && (
                  <Link
                    to="/lessons/$lessonId"
                    params={{ lessonId: card.lessonId }}
                    search={anchorSearch(cardAnchor(card))}
                    className="mt-6 inline-flex items-center gap-1.5 text-sm text-primary hover:underline"
                  >
                    <PlayCircle className="size-4" /> Jump to source
                    {anchorLabel(cardAnchor(card)) ? ` at ${anchorLabel(cardAnchor(card))}` : ""}
                    {card.lessonTitle ? ` · ${card.lessonTitle}` : ""}
                  </Link>
                )}
              </>
            ) : (
              <Button className="mt-8 w-full" variant="outline" onClick={() => setRevealed(true)}>
                Show answer <kbd className="ml-2 text-[10px] opacity-60">Space</kbd>
              </Button>
            )}
          </article>
          {revealed && (
            <div className="grid grid-cols-4 gap-2" role="group" aria-label="How well did you remember?">
              {GRADES.map((g) => (
                <button
                  key={g.grade}
                  onClick={() => grade(g.grade)}
                  title={g.hint}
                  className={cn(
                    "rounded-xl border bg-card p-3 text-center text-sm font-medium transition-colors",
                    g.tone,
                  )}
                >
                  {g.label}
                  <span className="mt-0.5 block text-[10px] font-normal text-muted-foreground">{g.key}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
