import { useMemo } from "react";
import { toast } from "sonner";
import { AlertCircle, Download as DownloadIcon, Pause, Play, RotateCcw, X } from "lucide-react";
import type { Lesson } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { QualityChip } from "./QualityChip";
import { QualityMenuItems } from "./QualityMenu";
import { formatBytes, formatEta, useDownloadsList } from "./useDownloads";
import { useLessonDownload } from "./useLessonDownload";

const act = (fn: () => Promise<unknown>) => fn().catch((e: unknown) => toast.error(errorMessage(e)));

/** Lesson header control: Download (with quality), live progress, or the downloaded quality. */
export function LessonDownloadControl({ lesson }: { lesson: Lesson }) {
  const { data: list } = useDownloadsList();
  const d = useMemo(() => list?.find((x) => x.lessonId === lesson.id), [list, lesson.id]);
  const start = useLessonDownload();

  if (lesson.localVideoPath)
    return <QualityChip quality={lesson.localQuality} offline className="py-1 text-xs" />;

  if (d && d.status !== "done") {
    const pct = Math.round((d.progress ?? 0) * 100);
    if (d.status === "error")
      return (
        <div className="flex items-center gap-1.5 rounded-md border border-warning/40 bg-warning/10 px-2 py-1 text-xs">
          <AlertCircle className="size-3.5 shrink-0 text-warning" aria-hidden />
          <span className="max-w-56 truncate" title={d.errorMessage ?? undefined}>
            {d.nextRetryAt ? "Download failed · retrying soon" : (d.errorMessage ?? "Download failed")}
          </span>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Retry download"
            onClick={() => act(() => commands.retryDownload(d.id))}
          >
            <RotateCcw />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Cancel download"
            onClick={() => act(() => commands.cancelDownload(d.id))}
          >
            <X />
          </Button>
        </div>
      );
    const label =
      d.status === "running"
        ? [`${pct}%`, d.speedBps ? `${formatBytes(d.speedBps)}/s` : "", formatEta(d.etaSec)]
            .filter(Boolean)
            .join(" · ")
        : d.status === "paused"
          ? `Paused · ${pct}%`
          : "Queued";
    return (
      <div
        className="relative flex items-center gap-1 overflow-hidden rounded-md border bg-card py-0.5 pl-2 pr-0.5 text-xs"
        aria-label={`Downloading at ${d.quality}: ${label}`}
      >
        <div
          className="absolute inset-y-0 left-0 bg-primary/15 transition-[width]"
          style={{ width: `${pct}%` }}
          aria-hidden
        />
        <DownloadIcon className="relative size-3.5 shrink-0 text-primary" aria-hidden />
        <span className="relative tabular-nums">
          {d.quality} · {label}
        </span>
        {d.status === "running" ? (
          <Button
            variant="ghost"
            size="icon-sm"
            className="relative"
            aria-label="Pause download"
            onClick={() => act(() => commands.pauseDownload(d.id))}
          >
            <Pause />
          </Button>
        ) : d.status === "paused" ? (
          <Button
            variant="ghost"
            size="icon-sm"
            className="relative"
            aria-label="Resume download"
            onClick={() => act(() => commands.resumeDownload(d.id))}
          >
            <Play />
          </Button>
        ) : null}
        <Button
          variant="ghost"
          size="icon-sm"
          className="relative"
          aria-label="Cancel download"
          onClick={() => act(() => commands.cancelDownload(d.id))}
        >
          <X />
        </Button>
      </div>
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm">
          <DownloadIcon /> Download
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <QualityMenuItems durationSec={lesson.durationSec} onPick={(q) => void start(lesson.id, q)} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
