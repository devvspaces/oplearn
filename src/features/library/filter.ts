import type { CourseSummary } from "@/bindings";

export const FILTERS = [
  ["all", "All"],
  ["in_progress", "In progress"],
  ["not_started", "Not started"],
  ["completed", "Completed"],
  ["archived", "Archived"],
] as const;

export type Filter = (typeof FILTERS)[number][0];
export type Sort = "recent" | "title" | "progress";

export function filterCourses(list: CourseSummary[], filter: Filter, sort: Sort): CourseSummary[] {
  const done = (c: CourseSummary) => c.lessonCount > 0 && c.completedCount === c.lessonCount;
  const out = list.filter((c) => {
    if (filter === "archived") return c.course.status === "archived";
    if (c.course.status === "archived") return false;
    if (filter === "completed") return done(c) || c.course.status === "completed";
    if (filter === "in_progress") return !done(c) && (c.completedCount > 0 || c.lastStudiedAt != null);
    if (filter === "not_started") return c.completedCount === 0 && c.lastStudiedAt == null;
    return true;
  });
  const pct = (c: CourseSummary) => (c.lessonCount ? c.completedCount / c.lessonCount : 0);
  return out.sort((a, b) =>
    sort === "title"
      ? a.course.title.localeCompare(b.course.title)
      : sort === "progress"
        ? pct(b) - pct(a)
        : (b.lastStudiedAt ?? b.course.updatedAt).localeCompare(a.lastStudiedAt ?? a.course.updatedAt),
  );
}
