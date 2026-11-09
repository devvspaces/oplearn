import { useEffect, useRef, useState } from "react";
import { Link, Navigate, createFileRoute, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import {
  ArrowLeft,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Circle,
  ExternalLink,
  Keyboard,
  Sparkles,
} from "lucide-react";
import type { Annotation, AnnotationType, AssistFocus } from "@/bindings";
import {
  commands,
  errorMessage,
  qk,
  useAnnotations,
  useChapters,
  useLessonContext,
  useLibraryMutation,
} from "@/lib/api";
import { useHotkeys } from "@/lib/hotkeys";
import { attachmentMarkdown } from "@/lib/attachments";
import { formatDuration, formatTimestamp } from "@/lib/time";
import { Button } from "@/components/ui/button";
import { Badge, Spinner } from "@/components/ui/misc";
import { StudyPlayer } from "@/features/player/StudyPlayer";
import { player } from "@/features/player/store";
import { AnnotationsPanel } from "@/features/annotations/AnnotationsPanel";
import { TYPE_META } from "@/features/annotations/timeline";
import { TranscriptPanel, TRANSCRIPT_SEARCH_ID } from "@/features/transcript/TranscriptPanel";
import { NotebookEditor } from "@/features/notebook/NotebookEditor";
import { LessonDownloadControl } from "@/features/downloads/LessonDownloadControl";
import { useNotesActivity } from "@/features/study/api";
import { LessonCards } from "@/features/flashcards/LessonCards";
import { MaterialViewer } from "@/features/materials/MaterialViewer";
import type { SelectionAction } from "@/features/materials/PdfViewer";
import { pdfNav } from "@/features/materials/pdf-nav";
import { useCardDialog } from "@/features/flashcards/api";
import { compareAnchors } from "@/lib/anchors";
import { AssistDialog } from "@/features/ai/AssistDialog";
import { kindLabel, useLessonMaterials } from "@/features/materials/api";
import { QuizPanel } from "@/features/quiz/QuizPanel";
import { useUi } from "@/stores/ui";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/lessons/$lessonId")({
  validateSearch: (s: Record<string, unknown>): { t?: number; doc?: string; page?: number; q?: string } => {
    const t = Number(s.t);
    const page = Number(s.page);
    return {
      ...(Number.isFinite(t) && t >= 0 ? { t } : {}),
      ...(typeof s.doc === "string" && s.doc ? { doc: s.doc } : {}),
      ...(Number.isFinite(page) && page >= 1 ? { page: Math.floor(page) } : {}),
      ...(typeof s.q === "string" && s.q ? { q: s.q } : {}),
    };
  },
  component: LessonPage,
});

const SHORTCUTS: Array<[string, string]> = [
  ["Space / K", "Play / pause"],
  ["J / L", "−10 s / +10 s"],
  ["← / →", "−5 s / +5 s"],
  ["[ / ]", "Speed down / up"],
  ["N B Q T", "Note, bookmark, question, to-do"],
  ["E", "Explain this moment with AI"],
  ["/", "Search transcript"],
  ["F", "Fullscreen"],
  ["M", "Mute"],
  ["⌘K", "Search everything"],
];

const TABS = [
  ["notes", "Notes"],
  ["transcript", "Transcript"],
  ["notebook", "Notebook"],
  ["cards", "Cards"],
  ["quiz", "Quiz"],
] as const;

function LessonPage() {
  const { lessonId } = Route.useParams();
  const { t: startAt, doc: docParam, page: pageParam, q: queryParam } = Route.useSearch();
  const { data, isLoading } = useLessonContext(lessonId);
  const { data: annotations } = useAnnotations(lessonId);
  const { data: chapters } = useChapters(lessonId);
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { panelTab, setPanelTab, panelWidth, setPanelWidth, pauseOnNote } = useUi();
  const [theatre, setTheatre] = useState(false);
  const [ended, setEnded] = useState(false);
  const [focusId, setFocusId] = useState<string | null>(null);
  const rateTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const complete = useLibraryMutation((c: boolean) => commands.setLessonCompleted(lessonId, c));
  useNotesActivity(data ? lessonId : null);
  const { data: lessonMaterials } = useLessonMaterials(data ? lessonId : null);

  const isVideo = data?.lesson.type === "video" && !!data.lesson.youtubeVideoId;

  // The file the player uses. A download that finishes mid-lesson doesn't swap the player
  // under the user; a toast offers the switch instead. Removing the file switches back to streaming.
  const localPath = data?.lesson.localVideoPath ?? null;
  const [playPath, setPlayPath] = useState<{ lessonId: string; path: string | null } | null>(null);
  const offered = useRef<string | null>(null);
  useEffect(() => {
    if (!data) return;
    const cur = playPath;
    if (!cur || cur.lessonId !== lessonId || !localPath) {
      if (cur?.lessonId !== lessonId || cur.path !== localPath) setPlayPath({ lessonId, path: localPath });
      return;
    }
    if (cur.path == null && offered.current !== localPath) {
      offered.current = localPath;
      toast.success("Download finished", {
        action: {
          label: "Switch to offline copy",
          onClick: () => setPlayPath({ lessonId, path: localPath }),
        },
      });
    }
  }, [data, lessonId, localPath, playPath]);
  const activePath = playPath?.lessonId === lessonId ? playPath.path : localPath;

  /** Saves the current frame (local files only) into a new note at that moment. */
  async function captureFrame(sec: number) {
    try {
      const at = Math.round(sec * 10) / 10;
      const note = await commands.createAnnotation({ lessonId, type: "note", startSec: at });
      const rel = await commands.captureFrame(note.id, at);
      const saved = await commands.updateAnnotation(note.id, {
        bodyMd: `${attachmentMarkdown(rel, `Frame ${formatTimestamp(at)}`)}\n\n`,
      });
      qc.setQueryData<Annotation[]>(qk.annotations(lessonId), (list) =>
        [...(list ?? []), saved].sort(compareAnchors),
      );
      void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
      setPanelTab("notes");
      setFocusId(saved.id);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  async function createAt(
    type: AnnotationType,
    opts?: { body?: string; start?: number; page?: { materialId: string; page: number } },
  ) {
    const start = isVideo ? Math.round((opts?.start ?? player.time()) * 10) / 10 : undefined;
    // Documents anchor to the page in view (or the page the selection came from).
    const doc = !isVideo ? (opts?.page ?? pdfNav.current()) : null;
    if (isVideo && pauseOnNote && type !== "bookmark") player.pause();
    try {
      const a = await commands.createAnnotation({
        lessonId,
        type,
        startSec: start,
        bodyMd: opts?.body,
        materialId: doc?.materialId,
        page: doc?.page,
      });
      qc.setQueryData<Annotation[]>(qk.annotations(lessonId), (list) =>
        [...(list ?? []), a].sort(compareAnchors),
      );
      void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
      setPanelTab("notes");
      if (type === "bookmark") {
        toast.success(doc ? `Bookmark on page ${doc.page}` : `Bookmark at ${formatTimestamp(start ?? 0)}`);
      } else {
        setFocusId(a.id);
      }
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  const [explain, setExplain] = useState<AssistFocus | null>(null);
  function openExplain() {
    if (isVideo) {
      player.pause();
      setExplain({ lessonId, sec: Math.round(player.time()) });
    } else {
      const doc = pdfNav.current();
      setExplain(doc ? { lessonId, materialId: doc.materialId, page: doc.page } : { lessonId });
    }
  }

  useHotkeys(
    {
      n: () => void createAt("note"),
      b: () => void createAt("bookmark"),
      q: () => void createAt("question"),
      t: () => void createAt("todo"),
      e: () => openExplain(),
      "/": () => {
        setPanelTab("transcript");
        requestAnimationFrame(() => document.getElementById(TRANSCRIPT_SEARCH_ID)?.focus());
      },
    },
    !!data,
  );

  if (isLoading)
    return (
      <div className="p-8">
        <Spinner />
      </div>
    );
  if (!data) return <div className="p-8 text-destructive">Lesson not found.</div>;
  const { lesson, course, moduleTitle, prevLessonId, nextLessonId } = data;
  const primaryMaterial = !isVideo
    ? (lessonMaterials ?? []).find((m) => !m.solutionOf && m.kind !== "video")
    : undefined;
  const lectureNotes = isVideo
    ? (lessonMaterials ?? []).filter((m) => m.kind !== "video" && !m.solutionOf)
    : [];
  const ocwVideo = isVideo ? (lessonMaterials ?? []).find((m) => m.kind === "video") : undefined;
  const primarySolutions = primaryMaterial
    ? (lessonMaterials ?? []).filter((m) => m.solutionOf === primaryMaterial.id)
    : [];
  // A page link into a document that isn't this lesson's own (e.g. lecture notes beside a
  // video) opens that document instead.
  if (
    docParam &&
    lessonMaterials &&
    docParam !== primaryMaterial?.id &&
    !primarySolutions.some((m) => m.id === docParam)
  )
    return (
      <Navigate
        to="/materials/$materialId"
        params={{ materialId: docParam }}
        search={{ page: pageParam, q: queryParam }}
        replace
      />
    );
  const isDoc = !!primaryMaterial;
  const onSelectText = (action: SelectionAction, text: string, page: number, materialId: string) => {
    if (action === "card") {
      useCardDialog
        .getState()
        .open({ lessonId, sourceMaterialId: materialId, sourcePage: page, front: "", back: text });
    } else {
      void createAt(action, { body: `> ${text}\n\n`, page: { materialId, page } });
    }
  };
  const done = lesson.completedAt != null;

  const go = (id: string | null) =>
    id && void navigate({ to: "/lessons/$lessonId", params: { lessonId: id } });
  const saveRate = (r: number) => {
    clearTimeout(rateTimer.current);
    rateTimer.current = setTimeout(() => void commands.updateCourse(course.id, { playbackRate: r }), 600);
  };

  const startDrag = (e: React.PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = panelWidth;
    const move = (ev: PointerEvent) => setPanelWidth(startW + (startX - ev.clientX));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const sidePanel = (
    <aside
      className="relative flex min-h-0 flex-col overflow-hidden rounded-xl border bg-card lg:sticky lg:top-6 lg:h-[calc(100vh-3rem)]"
      style={{ width: theatre ? undefined : panelWidth }}
    >
      {!theatre && (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize side panel"
          tabIndex={0}
          onPointerDown={startDrag}
          onKeyDown={(e) => {
            if (e.key === "ArrowLeft") setPanelWidth(panelWidth + 24);
            if (e.key === "ArrowRight") setPanelWidth(panelWidth - 24);
          }}
          className="absolute inset-y-0 left-0 z-10 w-1.5 cursor-col-resize hover:bg-primary/30 focus-visible:bg-primary/40"
        />
      )}
      <div role="tablist" aria-label="Study panel" className="flex gap-1 border-b p-1.5">
        {TABS.filter(([k]) => isVideo || k !== "transcript").map(([k, label]) => (
          <button
            key={k}
            role="tab"
            aria-selected={panelTab === k}
            onClick={() => setPanelTab(k)}
            className={cn(
              "flex-1 rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:text-foreground",
              panelTab === k && "bg-accent text-foreground",
            )}
          >
            {label}
            {k === "notes" && annotations?.length ? (
              <span className="ml-1 opacity-60">{annotations.length}</span>
            ) : null}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1">
        {panelTab === "transcript" && isVideo ? (
          <TranscriptPanel
            lessonId={lesson.id}
            onCreateFromSelection={(type, text, start) =>
              void createAt(type, { body: `> ${text}\n\n`, start })
            }
          />
        ) : panelTab === "quiz" ? (
          <QuizPanel scope="lesson" scopeId={lesson.id} courseId={course.id} compact />
        ) : panelTab === "cards" ? (
          <LessonCards lessonId={lesson.id} isVideo={isVideo} />
        ) : panelTab === "notebook" ? (
          <NotebookEditor scope="lesson" scopeId={lesson.id} placeholder="Free-form notes for this lesson…" />
        ) : (
          <AnnotationsPanel
            lessonId={lesson.id}
            courseTitle={course.title}
            focusId={focusId}
            onCreate={(t) => void createAt(t)}
          />
        )}
      </div>
    </aside>
  );

  return (
    <div className={cn("mx-auto flex flex-col gap-4 p-6", theatre ? "max-w-none" : "max-w-[1600px]")}>
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Link
          to="/courses/$courseId"
          params={{ courseId: course.id }}
          className="inline-flex items-center gap-1 hover:text-foreground"
        >
          <ArrowLeft className="size-4" /> {course.title}
        </Link>
        <span aria-hidden>/</span>
        <span className="truncate">{moduleTitle}</span>
      </div>

      <div className={cn("flex flex-col gap-6", !theatre && "lg:flex-row lg:items-start")}>
        <div className="min-w-0 flex-1">
          {isVideo ? (
            <StudyPlayer
              key={lesson.id}
              lessonId={lesson.id}
              videoId={lesson.youtubeVideoId!}
              lastPositionSec={lesson.lastPositionSec}
              durationSec={lesson.durationSec}
              startAt={startAt}
              localPath={activePath}
              onCaptureFrame={(t) => void captureFrame(t)}
              rate={course.playbackRate ?? 1}
              annotations={annotations ?? []}
              chapters={chapters ?? []}
              onRateChange={saveRate}
              onEnded={() => setEnded(true)}
              onAutoComplete={() => toast.success("Lesson complete: you've watched enough of it")}
              onMarkerClick={(a) => {
                setPanelTab("notes");
                setFocusId(null);
                requestAnimationFrame(() =>
                  document
                    .querySelector(`[data-annotation-id="${a.id}"]`)
                    ?.scrollIntoView({ block: "nearest", behavior: "smooth" }),
                );
              }}
            />
          ) : primaryMaterial ? (
            <MaterialViewer
              material={primaryMaterial}
              solutions={primarySolutions}
              page={docParam === primaryMaterial.id || !docParam ? pageParam : undefined}
              query={queryParam}
              onSelectText={onSelectText}
              className="h-[75vh]"
            />
          ) : (
            <div className="rounded-xl border bg-card p-6">
              <Badge tone="muted" className="capitalize">
                {lesson.type}
              </Badge>
              {lesson.description ? (
                <p className="mt-3 whitespace-pre-wrap text-sm">{lesson.description}</p>
              ) : (
                <p className="mt-3 text-sm text-muted-foreground">No description yet.</p>
              )}
              {lesson.url && (
                <Button variant="outline" className="mt-4" onClick={() => void openUrl(lesson.url!)}>
                  <ExternalLink /> Open link
                </Button>
              )}
            </div>
          )}

          <div className="mt-4 flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold leading-snug">{lesson.title}</h1>
              <p className="mt-1 text-xs text-muted-foreground">
                {[
                  lesson.docKind ? kindLabel(lesson.docKind) : null,
                  lesson.channelTitle,
                  lesson.docPages ? `${lesson.docPages} pages` : formatDuration(lesson.durationSec),
                ]
                  .filter(Boolean)
                  .join(" · ")}
                {lesson.license === "creativeCommon" && (
                  <Badge tone="success" className="ml-2">
                    CC
                  </Badge>
                )}
                {ocwVideo?.sourceUrl && (
                  <button
                    className="ml-2 text-primary hover:underline"
                    onClick={() => void openUrl(ocwVideo.sourceUrl!)}
                  >
                    Open on OCW
                  </button>
                )}
                {lesson.youtubeVideoId && (
                  <button
                    className="ml-2 text-primary hover:underline"
                    onClick={() => void openUrl(lesson.url ?? `https://youtu.be/${lesson.youtubeVideoId}`)}
                  >
                    youtu.be/{lesson.youtubeVideoId}
                  </button>
                )}
              </p>
              {lectureNotes.length > 0 && (
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {lectureNotes.map((m) => (
                    <Link
                      key={m.id}
                      to="/materials/$materialId"
                      params={{ materialId: m.id }}
                      className="rounded-md border px-2 py-0.5 text-xs hover:bg-accent"
                    >
                      {kindLabel(m.kind)}: {m.title}
                    </Link>
                  ))}
                </div>
              )}
            </div>
            <div className="flex items-center gap-2">
              {isVideo && <LessonDownloadControl lesson={lesson} />}
              {isVideo && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setTheatre((t) => !t)}
                  aria-pressed={theatre}
                >
                  Theatre
                </Button>
              )}
              <Button
                variant="outline"
                size="icon"
                aria-label="Previous lesson"
                disabled={!prevLessonId}
                onClick={() => go(prevLessonId)}
              >
                <ChevronLeft />
              </Button>
              <Button
                variant={done ? "secondary" : "default"}
                onClick={() => complete.mutate(!done)}
                aria-pressed={done}
              >
                {done ? <CheckCircle2 className="text-success" /> : <Circle />}
                {done ? "Completed" : "Mark complete"}
              </Button>
              <Button
                variant="outline"
                size="icon"
                aria-label="Next lesson"
                disabled={!nextLessonId}
                onClick={() => go(nextLessonId)}
              >
                <ChevronRight />
              </Button>
            </div>
          </div>
          {ended && nextLessonId && (
            <div className="mt-4 flex items-center justify-between rounded-lg border bg-card p-3 text-sm">
              <span>Finished this lecture.</span>
              <div className="flex gap-2">
                {!done && (
                  <Button size="sm" variant="secondary" onClick={() => complete.mutate(true)}>
                    Mark complete
                  </Button>
                )}
                <Button size="sm" onClick={() => go(nextLessonId)}>
                  Next lesson <ChevronRight />
                </Button>
              </div>
            </div>
          )}
          {(isVideo || isDoc) && (
            <div className="mt-4 flex flex-wrap gap-2">
              {(["note", "bookmark", "question", "todo"] as const).map((t) => (
                <Button key={t} variant="outline" size="sm" onClick={() => void createAt(t)}>
                  <span className="size-2 rounded-full" style={{ background: TYPE_META[t].color }} />
                  {TYPE_META[t].label}
                  <kbd className="ml-1 font-mono text-[10px] text-muted-foreground">{TYPE_META[t].key}</kbd>
                </Button>
              ))}
              <Button variant="outline" size="sm" onClick={openExplain}>
                <Sparkles className="text-primary" /> {isVideo ? "Explain this moment" : "Explain this page"}
                <kbd className="ml-1 font-mono text-[10px] text-muted-foreground">E</kbd>
              </Button>
            </div>
          )}
          {lesson.description && isVideo && (
            <details className="mt-4 rounded-lg border bg-card p-3 text-sm">
              <summary className="cursor-pointer text-muted-foreground">Description</summary>
              <p className="mt-2 whitespace-pre-wrap text-muted-foreground">{lesson.description}</p>
            </details>
          )}
          {isVideo && (
            <details className="mt-3 rounded-lg border bg-card p-3 text-sm">
              <summary className="flex cursor-pointer items-center gap-2 text-muted-foreground">
                <Keyboard className="size-4" /> Keyboard shortcuts
              </summary>
              <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
                {SHORTCUTS.map(([k, v]) => (
                  <div key={k} className="contents">
                    <dt>
                      <kbd className="rounded border bg-muted px-1 font-mono text-[10px]">{k}</kbd>
                    </dt>
                    <dd className="text-muted-foreground">{v}</dd>
                  </div>
                ))}
              </dl>
            </details>
          )}
        </div>
        {!theatre && sidePanel}
      </div>
      {theatre && <div className="h-[70vh]">{sidePanel}</div>}
      {explain && (
        <AssistDialog
          open={!!explain}
          onOpenChange={(o) => !o && setExplain(null)}
          kind="explain"
          focus={explain}
          askQuestion
          description={
            explain.sec != null
              ? `Claude explains what's happening around ${formatTimestamp(explain.sec)}, from the transcript and your notes.`
              : explain.page != null
                ? `Claude explains page ${explain.page} of this document.`
                : "Claude explains this lesson."
          }
          saveLabel="Save as question"
          onSave={async (ans, q) => {
            const where =
              explain.sec != null
                ? `at ${formatTimestamp(explain.sec)}`
                : explain.page != null
                  ? `on page ${explain.page}`
                  : "";
            const saved = await commands.saveExplanation(
              explain,
              q || `Explain ${where}`.trim(),
              ans.markdown,
            );
            qc.setQueryData<Annotation[]>(qk.annotations(lessonId), (list) =>
              [...(list ?? []), saved].sort(compareAnchors),
            );
            void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
            setPanelTab("notes");
            setFocusId(saved.id);
            toast.success("Saved as an answered question");
          }}
        />
      )}
    </div>
  );
}
