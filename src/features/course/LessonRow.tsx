import { Link } from "@tanstack/react-router";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertCircle,
  BookText,
  CheckCircle2,
  Circle,
  FlaskConical,
  GripVertical,
  MoreHorizontal,
  PlayCircle,
  Layers,
} from "lucide-react";
import type { Download, Lesson, Quality } from "@/bindings";
import { formatDuration, formatTimestamp } from "@/lib/time";
import { Badge } from "@/components/ui/misc";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { QualityChip } from "@/features/downloads/QualityChip";
import { QualityMenuItems } from "@/features/downloads/QualityMenu";
import { relativeDay, todayYmd } from "@/lib/dates";
import { cn } from "@/lib/utils";
import { kindIcon, kindLabel } from "@/features/materials/api";

const ICONS = { video: PlayCircle, reading: BookText, lab: FlaskConical, quiz: Layers } as const;

export function LessonRow({
  lesson,
  index,
  onToggleComplete,
  onRename,
  onDelete,
  download,
  onDownload,
  onRemoveDownload,
  onCancelDownload,
  plannedDate,
}: {
  lesson: Lesson;
  index: number;
  onToggleComplete: () => void;
  onRename: () => void;
  onDelete: () => void;
  download?: Download;
  onDownload: (quality: Quality) => void;
  onRemoveDownload: () => void;
  onCancelDownload: () => void;
  plannedDate?: string;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: lesson.id,
  });
  const Icon = lesson.docKind ? kindIcon(lesson.docKind) : ICONS[lesson.type];
  const label = lesson.docKind ? kindLabel(lesson.docKind) : lesson.type;
  const done = lesson.completedAt != null;
  const started = !done && (lesson.lastPositionSec ?? 0) > 0;

  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={cn(
        "group flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-accent/60",
        isDragging && "relative z-10 bg-accent shadow-lg",
      )}
    >
      <button
        {...attributes}
        {...listeners}
        aria-label={`Reorder ${lesson.title}`}
        className="cursor-grab touch-none text-muted-foreground/50 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
      >
        <GripVertical className="size-4" />
      </button>
      <button
        onClick={onToggleComplete}
        aria-label={done ? "Mark incomplete" : "Mark complete"}
        aria-pressed={done}
        className={cn("shrink-0", done ? "text-success" : "text-muted-foreground hover:text-foreground")}
      >
        {done ? <CheckCircle2 className="size-4.5" /> : <Circle className="size-4.5" />}
      </button>
      <span className="w-6 shrink-0 text-right text-xs tabular-nums text-muted-foreground">{index + 1}</span>
      <Icon className="size-4 shrink-0 text-muted-foreground" aria-label={label} />
      <Link
        to="/lessons/$lessonId"
        params={{ lessonId: lesson.id }}
        className={cn("min-w-0 flex-1 truncate text-sm hover:underline", done && "text-muted-foreground")}
      >
        {lesson.title}
      </Link>
      {started && (
        <Badge tone="primary" title="Resume position">
          {formatTimestamp(lesson.lastPositionSec!)}
        </Badge>
      )}
      {lesson.docKind && (
        <Badge tone="muted" className="hidden sm:inline-flex">
          {label}
        </Badge>
      )}
      {plannedDate && !done && <PlannedBadge date={plannedDate} />}
      <DownloadBadge lesson={lesson} download={download} />
      {lesson.license === "creativeCommon" && (
        <Badge tone="success" title="Creative Commons licensed">
          CC
        </Badge>
      )}
      <span className="w-16 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
        {lesson.docPages ? `${lesson.docPages} pp` : formatDuration(lesson.durationSec)}
      </span>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Actions for ${lesson.title}`}
            className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
          >
            <MoreHorizontal />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={onToggleComplete}>
            {done ? "Mark incomplete" : "Mark complete"}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onRename}>Rename…</DropdownMenuItem>
          {lesson.url && (
            <DropdownMenuItem onSelect={() => void openUrl(lesson.url!)}>
              Open on YouTube / web
            </DropdownMenuItem>
          )}
          {lesson.type === "video" &&
            lesson.youtubeVideoId &&
            (lesson.localVideoPath ? (
              <DropdownMenuItem onSelect={onRemoveDownload}>Remove download (keep notes)</DropdownMenuItem>
            ) : download && download.status !== "done" ? (
              <DropdownMenuItem onSelect={onCancelDownload}>Cancel download</DropdownMenuItem>
            ) : (
              <DropdownMenuSub>
                <DropdownMenuSubTrigger>Download for offline</DropdownMenuSubTrigger>
                <DropdownMenuSubContent>
                  <QualityMenuItems durationSec={lesson.durationSec} onPick={onDownload} />
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            ))}
          <DropdownMenuItem variant="destructive" onSelect={onDelete}>
            Delete lesson
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </li>
  );
}

function DownloadBadge({ lesson, download }: { lesson: Lesson; download?: Download }) {
  if (lesson.localVideoPath) return <QualityChip quality={lesson.localQuality} offline />;
  if (!download || download.status === "done") return null;
  if (download.status === "error")
    return (
      <span title={download.errorMessage ?? "Download failed"} className="text-warning">
        <AlertCircle className="size-3.5" aria-label="Download failed" />
      </span>
    );
  if (download.status === "running")
    return (
      <Badge tone="primary" title="Downloading">
        ↓ {Math.round((download.progress ?? 0) * 100)}%
      </Badge>
    );
  return (
    <Badge tone="muted" title={download.status === "paused" ? "Download paused" : "Waiting to download"}>
      ↓ {download.status === "paused" ? "paused" : "queued"}
    </Badge>
  );
}

function PlannedBadge({ date }: { date: string }) {
  const today = todayYmd();
  const overdue = date < today;
  return (
    <Badge tone={overdue ? "warning" : date === today ? "primary" : "muted"} title={`Planned for ${date}`}>
      {overdue ? `Due ${relativeDay(date)}` : relativeDay(date)}
    </Badge>
  );
}
