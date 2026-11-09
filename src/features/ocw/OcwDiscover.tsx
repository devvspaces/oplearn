import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { Download, ExternalLink, GraduationCap, Search as SearchIcon } from "lucide-react";
import type { OcwCourseCard, ImportProgress } from "@/bindings";
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
import { Badge, EmptyState, Progress, Spinner } from "@/components/ui/misc";
import { formatBytes } from "@/features/downloads/useDownloads";

const STAGE: Record<string, string> = {
  download: "Downloading course files",
  extract: "Unpacking",
  build: "Building lessons, modules and transcripts",
  index: "Indexing PDFs for search and AI",
  done: "Done",
};

/** Is the input a course link (or slug) rather than a search? */
const looksLikeCourse = (s: string) => /ocw\.mit\.edu\/courses\//.test(s);

export function OcwDiscover() {
  const [text, setText] = useState("");
  const [query, setQuery] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const results = useQuery({
    queryKey: ["ocwSearch", query],
    queryFn: () => commands.searchOcw(query!),
    enabled: !!query,
    staleTime: 10 * 60_000,
    retry: false,
  });

  return (
    <div className="pb-8 pt-4">
      <form
        className="mb-6 flex gap-2 rounded-xl border bg-card p-3"
        onSubmit={(e) => {
          e.preventDefault();
          const t = text.trim();
          if (!t) return;
          if (looksLikeCourse(t)) setPreview(t);
          else setQuery(t);
        }}
      >
        <div className="relative flex-1">
          <SearchIcon
            className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden
          />
          <Input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Search OpenCourseWare, or paste a course link (ocw.mit.edu/courses/…)"
            className="pl-8"
            aria-label="Search MIT OpenCourseWare"
          />
        </div>
        <Button type="submit" disabled={!text.trim()}>
          {looksLikeCourse(text) ? "Open" : "Search"}
        </Button>
      </form>

      {!query ? (
        <EmptyState
          icon={<GraduationCap className="size-8" />}
          title="Full MIT courses, ready to study offline"
        >
          Imports bring the syllabus, calendar, lecture videos with transcripts, lecture notes, readings,
          problem sets with solutions, recitations and exams. All MIT OpenCourseWare content is CC BY-NC-SA.
        </EmptyState>
      ) : results.isLoading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Spinner /> Searching…
        </div>
      ) : results.isError ? (
        <p className="text-sm text-destructive">{errorMessage(results.error)}</p>
      ) : !results.data?.length ? (
        <p className="text-sm text-muted-foreground">No OpenCourseWare courses matched.</p>
      ) : (
        <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {results.data.map((c) => (
            <CourseCard key={c.slug} c={c} onPreview={() => setPreview(c.slug)} />
          ))}
        </ul>
      )}
      <OcwPreviewDialog course={preview} onClose={() => setPreview(null)} />
    </div>
  );
}

function CourseCard({ c, onPreview }: { c: OcwCourseCard; onPreview: () => void }) {
  return (
    <li className="flex flex-col overflow-hidden rounded-xl border bg-card">
      <div className="aspect-video bg-muted">
        {c.imageUrl && <img src={c.imageUrl} alt="" loading="lazy" className="size-full object-cover" />}
      </div>
      <div className="flex flex-1 flex-col p-3">
        <p className="text-[11px] font-medium text-muted-foreground">
          {[c.number, c.term, c.level].filter(Boolean).join(" · ")}
        </p>
        <p className="mt-0.5 line-clamp-2 text-sm font-medium">{c.title}</p>
        {c.instructors.length > 0 && (
          <p className="mt-1 truncate text-xs text-muted-foreground">{c.instructors.join(", ")}</p>
        )}
        {c.features.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {c.features.slice(0, 4).map((f) => (
              <Badge key={f} tone="muted">
                {f}
              </Badge>
            ))}
          </div>
        )}
        <div className="mt-auto flex items-center gap-2 pt-3">
          <Badge tone="success" title="Creative Commons BY-NC-SA 4.0">
            CC
          </Badge>
          <Button size="sm" variant="outline" className="ml-auto" onClick={onPreview}>
            Preview & import
          </Button>
        </div>
      </div>
    </li>
  );
}

function OcwPreviewDialog({ course, onClose }: { course: string | null; onClose: () => void }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const q = useQuery({
    queryKey: ["ocwPreview", course],
    queryFn: () => commands.previewOcw(course!),
    enabled: !!course,
    retry: false,
  });
  const [prog, setProg] = useState<ImportProgress | null>(null);
  const busy = !!prog && prog.stage !== "done";
  const m = q.data?.meta;

  async function importIt() {
    if (!course) return;
    const ch = new Channel<ImportProgress>();
    ch.onmessage = (p) => setProg(p);
    setProg({ stage: "download", done: 0, total: 0, message: "Starting…" });
    try {
      const r = await commands.importOcw(course, ch);
      for (const k of [["courses"], ["course"]]) void qc.invalidateQueries({ queryKey: k });
      toast.success(`Imported ${r.lessons} lessons, ${r.materials} materials, ${r.transcripts} transcripts`, {
        description: r.pdfsIndexed ? `${r.pdfsIndexed} PDFs searchable` : undefined,
      });
      setProg(null);
      onClose();
      void navigate({ to: "/courses/$courseId", params: { courseId: r.courseId } });
    } catch (e) {
      toast.error(errorMessage(e));
      setProg(null);
    }
  }

  const pct = prog && prog.total ? (100 * (prog.done ?? 0)) / (prog.total ?? 1) : 0;
  return (
    <Dialog open={!!course} onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="max-h-[90vh] max-w-2xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle>{m?.title ?? "OpenCourseWare course"}</DialogTitle>
          <DialogDescription>
            {m
              ? [m.courseNumber, m.term, m.level.join(", ")].filter(Boolean).join(" · ")
              : "Loading course details…"}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-4 overflow-y-auto text-sm">
          {q.isLoading ? (
            <Spinner />
          ) : q.isError ? (
            <p className="text-destructive">{errorMessage(q.error)}</p>
          ) : m ? (
            <>
              <div className="flex gap-4">
                {q.data?.imageUrl && (
                  <img
                    src={q.data.imageUrl}
                    alt=""
                    className="hidden h-24 w-40 shrink-0 rounded-lg object-cover sm:block"
                  />
                )}
                <div className="min-w-0 space-y-1">
                  {m.instructors.length > 0 && <p>{m.instructors.join(", ")}</p>}
                  {m.topics.length > 0 && (
                    <p className="text-xs text-muted-foreground">{m.topics.slice(0, 4).join(" · ")}</p>
                  )}
                  <div className="flex flex-wrap gap-1 pt-1">
                    {m.resourceTypes.map((t) => (
                      <Badge key={t} tone="muted">
                        {t}
                      </Badge>
                    ))}
                  </div>
                </div>
              </div>
              {m.description && (
                <p className="line-clamp-6 whitespace-pre-wrap text-muted-foreground">{m.description}</p>
              )}
              <div className="rounded-lg border bg-muted/40 p-3 text-xs">
                <p>
                  Download: {q.data?.zipBytes ? <b>{formatBytes(q.data.zipBytes)}</b> : "size unknown"} (PDFs,
                  captions and pages). Lecture videos stream from YouTube; switch the course to Download mode
                  to save them too.
                </p>
                <p className="mt-1 text-muted-foreground">
                  © MIT OpenCourseWare, licensed{" "}
                  <button className="underline" onClick={() => void openUrl(m.license)}>
                    CC BY-NC-SA 4.0
                  </button>
                  . Imported for your personal study, with links back to the original.
                </p>
              </div>
              {prog && (
                <div role="status" className="rounded-lg border p-3 text-xs">
                  <div className="mb-1 flex justify-between">
                    <span>{STAGE[prog.stage] ?? prog.message}</span>
                    <span className="tabular-nums text-muted-foreground">
                      {prog.stage === "download" && prog.total
                        ? `${formatBytes(prog.done)} of ${formatBytes(prog.total)}`
                        : prog.total
                          ? `${Math.round(prog.done ?? 0)}/${Math.round(prog.total ?? 0)}`
                          : ""}
                    </span>
                  </div>
                  <Progress value={pct} label="Import progress" />
                </div>
              )}
            </>
          ) : null}
        </div>
        <DialogFooter>
          {m && (
            <Button variant="ghost" onClick={() => void openUrl(m.url)} disabled={busy}>
              <ExternalLink /> View on OCW
            </Button>
          )}
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Close
          </Button>
          <Button onClick={() => void importIt()} disabled={!m || busy || !q.data?.zipUrl}>
            {busy ? <Spinner /> : <Download />} {busy ? "Importing…" : "Import course"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
