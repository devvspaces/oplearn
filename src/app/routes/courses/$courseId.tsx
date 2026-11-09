import { useState } from "react";
import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";
import { ask, save } from "@tauri-apps/plugin-dialog";
import { Channel } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  Captions,
  Cloud,
  ExternalLink,
  FileDown,
  HardDrive,
  MoreHorizontal,
  Play,
  RefreshCw,
  Sparkles,
  Undo2,
} from "lucide-react";
import { DownloadDialog } from "@/features/downloads/DownloadDialog";
import { formatBytes } from "@/features/downloads/useDownloads";
import type { TranscriptProgress } from "@/bindings";
import { commands, errorMessage, mediaSrc, useCourse, useLibraryMutation } from "@/lib/api";
import { CourseNotebook } from "@/features/notebook/CourseNotebook";
import { cn } from "@/lib/utils";
import { formatDuration } from "@/lib/time";
import { Button } from "@/components/ui/button";
import { Badge, Progress, Spinner } from "@/components/ui/misc";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { CourseOutline } from "@/features/course/CourseOutline";
import { CourseFormDialog } from "@/features/library/CourseFormDialog";
import { PlanPanel } from "@/features/study/PlanPanel";
import { RestructureDialog } from "@/features/ai/RestructureDialog";
import { QuizPanel } from "@/features/quiz/QuizPanel";
import { AboutPanel, MaterialsPanel } from "@/features/materials/CoursePanels";
import { useCourseInfo } from "@/features/materials/api";
import { PlanStatusChip } from "@/features/study/PlanStatusChip";
import { useCoursePlan } from "@/features/study/api";

export const Route = createFileRoute("/courses/$courseId")({ component: CoursePage });

function CoursePage() {
  const { courseId } = Route.useParams();
  const { data, isLoading, error } = useCourse(courseId);
  const navigate = useNavigate();
  const [editOpen, setEditOpen] = useState(false);
  const update = useLibraryMutation((u: Parameters<typeof commands.updateCourse>[1]) =>
    commands.updateCourse(courseId, u),
  );
  const resync = useLibraryMutation(() => commands.resyncCourse(courseId));
  const remove = useLibraryMutation(() => commands.deleteCourse(courseId), { success: "Course deleted" });
  const qc = useQueryClient();
  const [tab, setTab] = useState<"outline" | "plan" | "quiz" | "about" | "materials" | "notebook">("outline");
  const { data: info } = useCourseInfo(courseId);
  const isProvider = info && info.provider !== "youtube";
  const { data: plan } = useCoursePlan(courseId);
  const [bulk, setBulk] = useState<TranscriptProgress | null>(null);
  const [downloadOpen, setDownloadOpen] = useState(false);
  const [restructureOpen, setRestructureOpen] = useState(false);
  const canUndo = useQuery({
    queryKey: ["canUndoRestructure", courseId],
    queryFn: () => commands.canUndoRestructure(courseId),
  });
  async function undoRestructure() {
    try {
      await commands.undoRestructure(courseId);
      for (const k of [["course"], ["courses"], ["canUndoRestructure"]])
        void qc.invalidateQueries({ queryKey: k });
      toast.success("Restored the previous outline");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  async function switchToStream() {
    if (
      !(await ask(
        "Switch to Stream mode? Pending downloads are cancelled; videos already downloaded are kept.",
        { kind: "info" },
      ))
    )
      return;
    try {
      await commands.setCourseMode(courseId, "stream", null);
      void qc.invalidateQueries({ queryKey: ["course"] });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  async function deleteLocal() {
    if (
      !(await ask("Delete all downloaded videos for this course? Notes, transcripts and progress are kept.", {
        kind: "warning",
      }))
    )
      return;
    try {
      const freed = await commands.deleteLocalFiles(courseId, null);
      toast.success(`Freed ${formatBytes(freed)}`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  async function exportMarkdown(title: string, slug: string) {
    const path = await save({
      title: `Export “${title}”`,
      defaultPath: `${slug}.md`,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!path) return;
    try {
      const written = await commands.exportCourseMarkdown(courseId, path);
      toast.success("Exported course notes", {
        description: written,
        action: { label: "Show", onClick: () => void revealItemInDir(written) },
      });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  async function fetchAllTranscripts() {
    const ch = new Channel<TranscriptProgress>();
    ch.onmessage = (p) => setBulk(p);
    setBulk({ done: 0, total: 0, lessonTitle: "", error: null });
    try {
      const r = await commands.fetchCourseTranscripts(courseId, ch);
      const parts = [`${r.fetched} fetched`];
      if (r.unavailable) parts.push(`${r.unavailable} without subtitles`);
      if (r.failed) parts.push(`${r.failed} failed`);
      (r.failed ? toast.warning : toast.success)(
        r.fetched + r.unavailable + r.failed === 0
          ? "All transcripts already fetched"
          : `Transcripts: ${parts.join(", ")}`,
      );
      void qc.invalidateQueries({ queryKey: ["transcript"] });
      void qc.invalidateQueries({ queryKey: ["chapters"] });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBulk(null);
    }
  }

  if (isLoading)
    return (
      <div className="p-8">
        <Spinner />
      </div>
    );
  if (error || !data) return <div className="p-8 text-destructive">Course not found.</div>;

  const { course, modules } = data;
  const all = modules.flatMap((m) => m.lessons);
  const done = all.filter((l) => l.completedAt).length;
  const totalSec = all.reduce((s, l) => s + (l.durationSec ?? 0), 0);
  const doneSec = all.filter((l) => l.completedAt).reduce((s, l) => s + (l.durationSec ?? 0), 0);
  const watched = all.filter((l) => !l.completedAt && (l.lastPositionSec ?? 0) > 0);
  const next =
    watched.sort((a, b) => (b.lastPositionSec ?? 0) - (a.lastPositionSec ?? 0))[0] ??
    all.find((l) => !l.completedAt);
  const ccCount = all.filter((l) => l.license === "creativeCommon").length;

  return (
    <div className="mx-auto max-w-5xl p-8">
      <Link
        to="/library"
        className="mb-4 inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
      >
        <ArrowLeft className="size-4" /> Library
      </Link>
      <header className="flex gap-6">
        {course.coverPath && (
          <img
            src={mediaSrc(course.coverPath)}
            alt=""
            className="hidden aspect-video w-64 shrink-0 rounded-xl object-cover md:block"
          />
        )}
        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-3">
            <h1 className="text-2xl font-semibold leading-tight">{course.title}</h1>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon" aria-label="Course actions">
                  <MoreHorizontal />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setEditOpen(true)}>Edit details…</DropdownMenuItem>
                {course.youtubePlaylistId && (
                  <DropdownMenuItem
                    onSelect={() =>
                      resync.mutate(undefined, {
                        onSuccess: (r) =>
                          void import("sonner").then(({ toast }) =>
                            toast.success(r.added ? `Added ${r.added} new lessons` : "Already up to date"),
                          ),
                      })
                    }
                  >
                    <RefreshCw /> Re-sync playlist
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem onSelect={() => setRestructureOpen(true)}>
                  <Sparkles /> Restructure with AI…
                </DropdownMenuItem>
                {canUndo.data && (
                  <DropdownMenuItem onSelect={() => void undoRestructure()}>
                    <Undo2 /> Undo AI restructure
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem onSelect={() => void exportMarkdown(course.title, course.slug)}>
                  <FileDown /> Export Markdown…
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => void deleteLocal()}>
                  <HardDrive /> Delete downloaded videos…
                </DropdownMenuItem>
                <DropdownMenuItem disabled={!!bulk} onSelect={() => void fetchAllTranscripts()}>
                  <Captions /> Fetch all transcripts
                </DropdownMenuItem>
                {course.sourceUrl && (
                  <DropdownMenuItem onSelect={() => void openUrl(course.sourceUrl!)}>
                    <ExternalLink /> Open source
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem
                  onSelect={() =>
                    update.mutate({ status: course.status === "archived" ? "active" : "archived" })
                  }
                >
                  {course.status === "archived" ? "Unarchive" : "Archive"}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={async () => {
                    if (
                      await ask(
                        `Delete “${course.title}” and all its notes and progress? This can't be undone.`,
                        { title: "Delete course", kind: "warning" },
                      )
                    )
                      remove.mutate(undefined, { onSuccess: () => void navigate({ to: "/library" }) });
                  }}
                >
                  Delete course
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
          {course.instructor && <p className="mt-1 text-sm text-muted-foreground">{course.instructor}</p>}
          {course.description && (
            <p className="mt-3 line-clamp-3 text-sm text-muted-foreground">{course.description}</p>
          )}
          <div className="mt-3 flex flex-wrap gap-1.5">
            {course.status !== "active" && (
              <Badge tone="muted" className="capitalize">
                {course.status}
              </Badge>
            )}
            {ccCount > 0 && (
              <Badge tone="success">
                {ccCount === all.length ? "Creative Commons" : `${ccCount} CC lessons`}
              </Badge>
            )}
            <PlanStatusChip status={plan?.status} />
            {course.tags.map((t) => (
              <Badge key={t}>{t}</Badge>
            ))}
          </div>
          <div className="mt-4 max-w-md">
            <div className="mb-1 flex justify-between text-xs text-muted-foreground">
              <span>
                {done}/{all.length} lessons
              </span>
              <span>
                {formatDuration(doneSec) || "0m"} of {formatDuration(totalSec) || "0m"}
              </span>
            </div>
            <Progress value={all.length ? (100 * done) / all.length : 0} label="Course progress" />
          </div>
          <div className="mt-4 flex flex-wrap items-center gap-3">
            {next && (
              <Button asChild>
                <Link to="/lessons/$lessonId" params={{ lessonId: next.id }}>
                  <Play /> {done === 0 && !watched.length ? "Start" : "Continue"}:{" "}
                  <span className="max-w-72 truncate">{next.title}</span>
                </Link>
              </Button>
            )}
            <div role="radiogroup" aria-label="Playback mode" className="flex gap-1 rounded-lg bg-muted p-1">
              <button
                role="radio"
                aria-checked={course.mode === "stream"}
                onClick={() => course.mode !== "stream" && void switchToStream()}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium text-muted-foreground",
                  course.mode === "stream" && "bg-background text-foreground shadow-sm",
                )}
                title="Play from YouTube (needs internet)"
              >
                <Cloud className="size-3.5" /> Stream
              </button>
              <button
                role="radio"
                aria-checked={course.mode === "download"}
                onClick={() => setDownloadOpen(true)}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium text-muted-foreground",
                  course.mode === "download" && "bg-background text-foreground shadow-sm",
                )}
                title="Save lessons for offline study"
              >
                <HardDrive className="size-3.5" /> Download
              </button>
            </div>
            {(() => {
              const offline = all.filter((l) => l.localVideoPath).length;
              const videos = all.filter((l) => l.type === "video").length;
              return offline > 0 ? (
                <span className="text-xs text-muted-foreground">
                  {offline}/{videos} available offline
                </span>
              ) : null;
            })()}
          </div>
        </div>
      </header>

      {bulk && (
        <div role="status" className="mt-6 rounded-lg border bg-card p-3 text-xs">
          <div className="flex justify-between text-muted-foreground">
            <span>Fetching transcripts{bulk.lessonTitle ? ` · ${bulk.lessonTitle}` : "…"}</span>
            <span className="tabular-nums">{bulk.total ? `${bulk.done}/${bulk.total}` : ""}</span>
          </div>
          <Progress
            className="mt-2"
            value={bulk.total ? (100 * bulk.done) / bulk.total : 0}
            label="Transcript fetch progress"
          />
          {bulk.error && <p className="mt-1 text-destructive">{bulk.error}</p>}
        </div>
      )}

      <div role="tablist" aria-label="Course sections" className="mb-4 mt-10 flex gap-1 border-b">
        {(isProvider
          ? (["outline", "plan", "quiz", "about", "materials", "notebook"] as const)
          : (["outline", "plan", "quiz", "notebook"] as const)
        ).map((t) => (
          <button
            key={t}
            role="tab"
            aria-selected={tab === t}
            onClick={() => setTab(t)}
            className={cn(
              "-mb-px border-b-2 border-transparent px-3 py-2 text-sm font-medium capitalize text-muted-foreground hover:text-foreground",
              tab === t && "border-primary text-foreground",
            )}
          >
            {t}
          </button>
        ))}
      </div>
      {tab === "outline" ? (
        <CourseOutline detail={data} plan={plan?.items} />
      ) : tab === "plan" ? (
        <PlanPanel courseId={courseId} />
      ) : tab === "quiz" ? (
        <div className="rounded-xl border bg-card">
          <QuizPanel scope="course" scopeId={courseId} courseId={courseId} />
        </div>
      ) : tab === "about" ? (
        <AboutPanel courseId={courseId} />
      ) : tab === "materials" ? (
        <MaterialsPanel courseId={courseId} />
      ) : (
        <CourseNotebook courseId={courseId} />
      )}

      <DownloadDialog courseId={courseId} open={downloadOpen} onOpenChange={setDownloadOpen} />
      <RestructureDialog detail={data} open={restructureOpen} onOpenChange={setRestructureOpen} />
      <CourseFormDialog
        open={editOpen}
        onOpenChange={setEditOpen}
        initial={course}
        title="Edit course"
        onSubmit={(v) => update.mutateAsync(v)}
      />
    </div>
  );
}
