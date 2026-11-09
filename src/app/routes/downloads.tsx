import { Link, createFileRoute } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import {
  AlertCircle,
  CheckCircle2,
  Download as DownloadIcon,
  HardDrive,
  Pause,
  Play,
  RotateCw,
  Trash2,
  X,
} from "lucide-react";
import type { Download } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Badge, EmptyState, Progress, Spinner } from "@/components/ui/misc";
import { QualityChip } from "@/features/downloads/QualityChip";
import { downloadsKey, formatBytes, formatEta, useDownloadsList } from "@/features/downloads/useDownloads";

export const Route = createFileRoute("/downloads")({ component: DownloadsPage });

const STATUS: Record<
  Download["status"],
  { label: string; tone: "primary" | "muted" | "warning" | "success" | "default" }
> = {
  running: { label: "Downloading", tone: "primary" },
  queued: { label: "Queued", tone: "muted" },
  paused: { label: "Paused", tone: "warning" },
  error: { label: "Failed", tone: "warning" },
  done: { label: "Downloaded", tone: "success" },
};

function act(fn: () => Promise<unknown>) {
  fn().catch((e: unknown) => toast.error(errorMessage(e)));
}

function DownloadRow({ d }: { d: Download }) {
  const s = STATUS[d.status];
  const pct = Math.round((d.progress ?? 0) * 100);
  const retrying = d.status === "error" && d.nextRetryAt;
  return (
    <li className="flex items-center gap-3 rounded-lg border bg-card p-3">
      <div className="shrink-0">
        {d.status === "done" ? (
          <CheckCircle2 className="size-5 text-success" aria-hidden />
        ) : d.status === "error" ? (
          <AlertCircle className="size-5 text-warning" aria-hidden />
        ) : d.status === "running" ? (
          <Spinner className="size-4 text-primary" />
        ) : (
          <DownloadIcon className="size-5 text-muted-foreground" aria-hidden />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <Link
            to="/lessons/$lessonId"
            params={{ lessonId: d.lessonId }}
            className="truncate text-sm font-medium hover:underline"
          >
            {d.lessonTitle}
          </Link>
          <Badge tone={s.tone}>{s.label}</Badge>
          {d.status === "done" && d.localQuality ? (
            <QualityChip quality={d.localQuality} />
          ) : (
            <span className="text-[11px] text-muted-foreground" title="Requested quality">
              {d.quality}
            </span>
          )}
        </div>
        <Link
          to="/courses/$courseId"
          params={{ courseId: d.courseId }}
          className="block truncate text-xs text-muted-foreground hover:underline"
        >
          {d.courseTitle}
        </Link>
        {(d.status === "running" ||
          d.status === "paused" ||
          (d.status === "queued" && (d.progress ?? 0) > 0)) && (
          <div className="mt-2">
            <Progress value={pct} label={`${d.lessonTitle} progress`} />
            <div className="mt-1 flex gap-3 text-[11px] tabular-nums text-muted-foreground">
              <span>{pct}%</span>
              {d.bytesDone != null && (
                <span>
                  {formatBytes(d.bytesDone)}
                  {d.bytesTotal ? ` of ${formatBytes(d.bytesTotal)}` : ""}
                </span>
              )}
              {d.status === "running" && d.speedBps ? <span>{formatBytes(d.speedBps)}/s</span> : null}
              {d.status === "running" && <span>{formatEta(d.etaSec)}</span>}
            </div>
          </div>
        )}
        {d.status === "error" && (
          <p className="mt-1 text-xs text-warning">
            {d.errorMessage}
            {retrying ? " · will retry automatically" : ""}
          </p>
        )}
        {d.status === "done" && d.bytesTotal ? (
          <p className="mt-0.5 text-[11px] text-muted-foreground">{formatBytes(d.bytesTotal)}</p>
        ) : null}
      </div>
      <div className="flex shrink-0 gap-1">
        {d.status === "running" && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Pause"
            onClick={() => act(() => commands.pauseDownload(d.id))}
          >
            <Pause />
          </Button>
        )}
        {d.status === "paused" && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Resume"
            onClick={() => act(() => commands.resumeDownload(d.id))}
          >
            <Play />
          </Button>
        )}
        {d.status === "error" && (
          <Button variant="ghost" size="sm" onClick={() => act(() => commands.retryDownload(d.id))}>
            <RotateCw /> Retry
          </Button>
        )}
        {d.status !== "done" && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Cancel download"
            onClick={() => act(() => commands.cancelDownload(d.id))}
          >
            <X />
          </Button>
        )}
      </div>
    </li>
  );
}

function DownloadsPage() {
  const { data, isLoading } = useDownloadsList();
  const qc = useQueryClient();
  const storage = useQuery({ queryKey: ["storage"], queryFn: commands.getStorageUsage });
  const list = data ?? [];
  const active = list.filter((d) => d.status !== "done");
  const done = list.filter((d) => d.status === "done");

  return (
    <div className="mx-auto max-w-4xl space-y-8 p-8">
      <div>
        <h1 className="text-2xl font-semibold">Downloads</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Lessons saved for offline study. Only download content you have the right to, such as Creative
          Commons lectures.
        </p>
      </div>

      <section>
        <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-muted-foreground">Queue</h2>
        {isLoading ? (
          <Spinner />
        ) : active.length === 0 ? (
          <EmptyState icon={<DownloadIcon className="size-8" />} title="Nothing downloading">
            Switch a course to <strong>Download</strong> mode, or download single lessons from the course
            page.
          </EmptyState>
        ) : (
          <ul className="space-y-2">
            {active.map((d) => (
              <DownloadRow key={d.id} d={d} />
            ))}
          </ul>
        )}
      </section>

      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">Storage</h2>
          {storage.data && (
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <HardDrive className="size-3.5" /> {formatBytes(storage.data.totalBytes) || "0 B"} in{" "}
              {storage.data.mediaDir}
            </span>
          )}
        </div>
        {storage.data?.courses.length ? (
          <ul className="divide-y rounded-lg border bg-card">
            {storage.data.courses.map((c) => (
              <li key={c.courseId} className="flex items-center gap-3 px-3 py-2.5 text-sm">
                <Link
                  to="/courses/$courseId"
                  params={{ courseId: c.courseId }}
                  className="min-w-0 flex-1 truncate hover:underline"
                >
                  {c.courseTitle}
                </Link>
                <span className="text-xs text-muted-foreground">{c.downloadedLessons} lessons</span>
                <span className="w-20 text-right text-xs tabular-nums">{formatBytes(c.bytes)}</span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Delete local files for ${c.courseTitle}`}
                  onClick={async () => {
                    if (
                      !(await ask(
                        `Delete downloaded videos for “${c.courseTitle}”? Notes, transcripts and progress are kept.`,
                        { kind: "warning" },
                      ))
                    )
                      return;
                    try {
                      const freed = await commands.deleteLocalFiles(c.courseId, null);
                      toast.success(`Freed ${formatBytes(freed)}`);
                      void qc.invalidateQueries({ queryKey: ["storage"] });
                      void qc.invalidateQueries({ queryKey: downloadsKey });
                    } catch (e) {
                      toast.error(errorMessage(e));
                    }
                  }}
                >
                  <Trash2 />
                </Button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted-foreground">No downloaded media yet.</p>
        )}
      </section>

      {done.length > 0 && (
        <section>
          <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-muted-foreground">
            Completed ({done.length})
          </h2>
          <ul className="space-y-2">
            {done.slice(0, 50).map((d) => (
              <DownloadRow key={d.id} d={d} />
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
