import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronUp, Layers, MessageCircleQuestion, NotebookPen, RefreshCw } from "lucide-react";
import { useCardDialog } from "@/features/flashcards/api";
import type { AnnotationType, Chapter } from "@/bindings";
import { commands, errorMessage, qk, useChapters, useTranscript } from "@/lib/api";
import { formatTimestamp } from "@/lib/time";
import { player, usePlayer } from "@/features/player/store";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/misc";
import { currentCueIndex, findCues } from "./cues";
import { cn } from "@/lib/utils";

export const TRANSCRIPT_SEARCH_ID = "transcript-search";

export function TranscriptPanel({
  lessonId,
  onCreateFromSelection,
}: {
  lessonId: string;
  onCreateFromSelection: (type: AnnotationType, text: string, startSec: number) => void;
}) {
  const qc = useQueryClient();
  const { data: transcript, isLoading } = useTranscript(lessonId);
  const { data: chapters } = useChapters(lessonId);
  const fetchT = useMutation({
    mutationFn: () => commands.fetchTranscript(lessonId),
    onSuccess: (t) => {
      qc.setQueryData(qk.transcript(lessonId), t);
      void qc.invalidateQueries({ queryKey: qk.chapters(lessonId) });
    },
  });

  // Fetch automatically the first time a lesson is opened.
  const autoFetched = useRef<string | null>(null);
  useEffect(() => {
    if (transcript && transcript.source == null && autoFetched.current !== lessonId && !fetchT.isPending) {
      autoFetched.current = lessonId;
      fetchT.mutate();
    }
  }, [transcript, lessonId, fetchT]);

  const cues = useMemo(() => transcript?.cues ?? [], [transcript]);
  const time = usePlayer((s) => s.time);
  const current = currentCueIndex(cues, time);

  const [query, setQuery] = useState("");
  const matches = useMemo(() => findCues(cues, query), [cues, query]);
  const [matchPos, setMatchPos] = useState(0);
  useEffect(() => setMatchPos(0), [query]);

  const listRef = useRef<HTMLDivElement>(null);
  const userScrollAt = useRef(0);
  const scrollToCue = (i: number, behavior: ScrollBehavior = "smooth") =>
    listRef.current?.querySelector(`[data-cue="${i}"]`)?.scrollIntoView({ block: "center", behavior });

  // Follow playback unless the user scrolled in the last few seconds or is searching.
  useEffect(() => {
    if (current < 0 || query || Date.now() - userScrollAt.current < 4000) return;
    scrollToCue(current);
  }, [current, query]);

  useEffect(() => {
    const i = matches[matchPos];
    if (i != null) scrollToCue(i);
  }, [matches, matchPos]);

  // Selection → "Note / Question" toolbar.
  const [sel, setSel] = useState<{ text: string; start: number; top: number; left: number } | null>(null);
  const onMouseUp = () => {
    const s = window.getSelection();
    const text = s?.toString().trim() ?? "";
    if (!s || !text || !listRef.current || !s.anchorNode || !listRef.current.contains(s.anchorNode))
      return setSel(null);
    const el = (s.anchorNode instanceof Element ? s.anchorNode : s.anchorNode.parentElement)?.closest(
      "[data-cue]",
    );
    const idx = el ? Number(el.getAttribute("data-cue")) : -1;
    const cue = cues[idx];
    if (!cue) return setSel(null);
    const rect = s.getRangeAt(0).getBoundingClientRect();
    const box = listRef.current.getBoundingClientRect();
    setSel({
      text,
      start: cue.start ?? 0,
      top: rect.top - box.top + listRef.current.scrollTop - 36,
      left: Math.max(4, rect.left - box.left),
    });
  };

  const chapterAt = useMemo(
    () =>
      chapterStarts(
        chapters ?? [],
        cues.map((c) => c.start ?? 0),
      ),
    [chapters, cues],
  );

  if (isLoading)
    return (
      <div className="p-4">
        <Spinner />
      </div>
    );

  const fetching = fetchT.isPending;
  const header = (
    <div className="flex items-center gap-1.5 border-b px-2 py-1.5">
      <Input
        id={TRANSCRIPT_SEARCH_ID}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && matches.length) {
            e.preventDefault();
            setMatchPos((p) => (p + (e.shiftKey ? matches.length - 1 : 1)) % matches.length);
          }
          if (e.key === "Escape") {
            setQuery("");
            e.currentTarget.blur();
          }
        }}
        placeholder="Search transcript ( / )"
        className="h-7 text-xs"
        disabled={!cues.length}
      />
      {query && (
        <>
          <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
            {matches.length ? `${matchPos + 1}/${matches.length}` : "0"}
          </span>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Previous match"
            onClick={() => setMatchPos((p) => (p + matches.length - 1) % Math.max(1, matches.length))}
          >
            <ChevronUp />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Next match"
            onClick={() => setMatchPos((p) => (p + 1) % Math.max(1, matches.length))}
          >
            <ChevronDown />
          </Button>
        </>
      )}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Re-fetch transcript"
        title="Re-fetch transcript"
        disabled={fetching}
        onClick={() => fetchT.mutate()}
      >
        <RefreshCw className={cn(fetching && "animate-spin")} />
      </Button>
    </div>
  );

  let body: React.ReactNode;
  if (fetching && !cues.length) {
    body = (
      <div className="flex flex-col items-center gap-2 p-8 text-center text-xs text-muted-foreground">
        <Spinner />
        <p>Fetching the transcript from YouTube…</p>
        <p>This can take up to a minute. You can keep watching.</p>
      </div>
    );
  } else if (fetchT.isError && !cues.length) {
    body = (
      <div className="flex flex-col items-center gap-2 p-8 text-center text-xs">
        <p className="text-destructive">{errorMessage(fetchT.error)}</p>
        <Button size="sm" variant="outline" onClick={() => fetchT.mutate()}>
          Try again
        </Button>
      </div>
    );
  } else if (transcript?.source === "none" || !cues.length) {
    body = (
      <div className="flex flex-col items-center gap-2 p-8 text-center text-xs text-muted-foreground">
        <p>No subtitles are available for this video.</p>
        <Button size="sm" variant="outline" onClick={() => fetchT.mutate()}>
          Check again
        </Button>
      </div>
    );
  } else {
    const matchSet = new Set(matches);
    const activeMatch = matches[matchPos];
    body = (
      <div
        ref={listRef}
        className="relative min-h-0 flex-1 overflow-y-auto px-1 py-2"
        onWheel={() => (userScrollAt.current = Date.now())}
        onPointerDown={() => (userScrollAt.current = Date.now())}
        onMouseUp={onMouseUp}
      >
        {cues.map((c, i) => (
          <Fragment key={i}>
            {chapterAt.get(i) && (
              <div className="sticky top-0 z-[1] mt-2 bg-background/95 px-2 py-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground backdrop-blur">
                {chapterAt.get(i)}
              </div>
            )}
            <div
              data-cue={i}
              aria-current={i === current ? "true" : undefined}
              className={cn(
                "group flex cursor-pointer gap-2 rounded px-2 py-1 text-sm leading-snug hover:bg-accent/60",
                i === current && "bg-primary/10 text-foreground",
                i !== current && "text-muted-foreground",
                matchSet.has(i) && "bg-warning/10",
                i === activeMatch && "ring-1 ring-warning",
              )}
              onClick={() => {
                if (!window.getSelection()?.toString()) player.seek(c.start ?? 0);
              }}
            >
              <span className="w-12 shrink-0 select-none pt-0.5 text-right font-mono text-[10px] text-muted-foreground/70 group-hover:text-primary">
                {formatTimestamp(c.start ?? 0)}
              </span>
              <span>{c.text}</span>
            </div>
          </Fragment>
        ))}
        {sel && (
          <div
            className="absolute z-10 flex gap-1 rounded-md border bg-popover p-1 shadow-lg"
            style={{ top: sel.top, left: sel.left }}
          >
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-xs"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                onCreateFromSelection("note", sel.text, sel.start);
                setSel(null);
                window.getSelection()?.removeAllRanges();
              }}
            >
              <NotebookPen /> Note
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-xs"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                onCreateFromSelection("question", sel.text, sel.start);
                setSel(null);
                window.getSelection()?.removeAllRanges();
              }}
            >
              <MessageCircleQuestion /> Question
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-xs"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                useCardDialog.getState().open({ lessonId, sourceSec: sel.start, front: "", back: sel.text });
                setSel(null);
                window.getSelection()?.removeAllRanges();
              }}
            >
              <Layers /> Card
            </Button>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      {header}
      {body}
      {transcript?.source && transcript.source !== "none" && (
        <div className="border-t px-3 py-1 text-[10px] text-muted-foreground">
          {transcript.source === "auto" ? "Auto-generated captions" : "Subtitles"} · {transcript.lang}
        </div>
      )}
    </div>
  );
}

/** Maps cue index → chapter title for the first cue at or after each chapter start. */
function chapterStarts(chapters: Chapter[], starts: number[]): Map<number, string> {
  const m = new Map<number, string>();
  if (chapters.length < 2) return m;
  for (const ch of chapters) {
    const i = starts.findIndex((s) => s >= (ch.startSec ?? 0) - 0.5);
    if (i >= 0 && !m.has(i)) m.set(i, ch.title);
  }
  return m;
}
