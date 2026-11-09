import { Link } from "@tanstack/react-router";
import { GraduationCap } from "lucide-react";
import type { CourseSummary } from "@/bindings";
import { mediaSrc } from "@/lib/api";
import { formatDuration } from "@/lib/time";
import { Badge, ProgressRing } from "@/components/ui/misc";

export function CourseCard({ summary }: { summary: CourseSummary }) {
  const { course, lessonCount, completedCount, totalDurationSec, nextLessonTitle } = summary;
  const pct = lessonCount ? (100 * completedCount) / lessonCount : 0;
  return (
    <Link
      to="/courses/$courseId"
      params={{ courseId: course.id }}
      className="group flex flex-col overflow-hidden rounded-xl border bg-card transition-colors hover:border-primary/50"
    >
      <div className="relative aspect-video bg-muted">
        {course.coverPath ? (
          <img src={mediaSrc(course.coverPath)} alt="" className="size-full object-cover" loading="lazy" />
        ) : (
          <div className="flex size-full items-center justify-center text-muted-foreground">
            <GraduationCap className="size-10" aria-hidden />
          </div>
        )}
        {course.status !== "active" && (
          <Badge tone="muted" className="absolute left-2 top-2 capitalize">
            {course.status}
          </Badge>
        )}
      </div>
      <div className="flex flex-1 gap-3 p-3">
        <div className="min-w-0 flex-1">
          <h3 className="line-clamp-2 text-sm font-semibold leading-snug">{course.title}</h3>
          {course.instructor && (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">{course.instructor}</p>
          )}
          <p className="mt-2 text-xs text-muted-foreground">
            {lessonCount} lessons{totalDurationSec > 0 && ` · ${formatDuration(totalDurationSec)}`}
          </p>
          {nextLessonTitle && pct < 100 && (
            <p className="mt-1 truncate text-xs">
              <span className="text-muted-foreground">Next:</span> {nextLessonTitle}
            </p>
          )}
        </div>
        <div className="relative shrink-0 self-start">
          <ProgressRing value={pct} />
          <span className="absolute inset-0 flex items-center justify-center text-[10px] font-medium">
            {Math.round(pct)}%
          </span>
        </div>
      </div>
    </Link>
  );
}
