import { useMemo } from "react";
import { Link } from "@tanstack/react-router";
import type { AnnotationInContext } from "@/bindings";
import { useCourseAnnotations } from "@/lib/api";
import { EmptyState, Spinner } from "@/components/ui/misc";
import { TYPE_META } from "@/features/annotations/timeline";
import { NotebookEditor } from "./NotebookEditor";
import { anchorLabel, anchorSearch } from "@/lib/anchors";

interface LessonGroup {
  lessonId: string;
  lessonTitle: string;
  items: AnnotationInContext[];
}
interface ModuleGroup {
  moduleId: string;
  moduleTitle: string;
  lessons: LessonGroup[];
}

export function groupByModule(list: AnnotationInContext[]): ModuleGroup[] {
  const out: ModuleGroup[] = [];
  for (const a of list) {
    let m = out[out.length - 1];
    if (!m || m.moduleId !== a.moduleId)
      out.push((m = { moduleId: a.moduleId, moduleTitle: a.moduleTitle, lessons: [] }));
    let l = m.lessons[m.lessons.length - 1];
    if (!l || l.lessonId !== a.annotation.lessonId)
      m.lessons.push((l = { lessonId: a.annotation.lessonId, lessonTitle: a.lessonTitle, items: [] }));
    l.items.push(a);
  }
  return out;
}

/** Course-wide notebook: every annotation grouped by module and lesson, plus a free-form course notebook. */
export function CourseNotebook({ courseId }: { courseId: string }) {
  const { data, isLoading } = useCourseAnnotations(courseId);
  const groups = useMemo(() => groupByModule(data ?? []), [data]);

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
      <section className="min-w-0">
        <h3 className="mb-2 text-sm font-semibold">All annotations</h3>
        {isLoading ? (
          <Spinner />
        ) : groups.length === 0 ? (
          <EmptyState title="No annotations yet">
            Notes, bookmarks, questions and to-dos you add while watching show up here.
          </EmptyState>
        ) : (
          <div className="space-y-5">
            {groups.map((m) => (
              <div key={m.moduleId}>
                <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {m.moduleTitle}
                </h4>
                <div className="space-y-3">
                  {m.lessons.map((l) => (
                    <div key={l.lessonId} className="rounded-xl border bg-card p-3">
                      <Link
                        to="/lessons/$lessonId"
                        params={{ lessonId: l.lessonId }}
                        className="text-sm font-medium hover:underline"
                      >
                        {l.lessonTitle}
                      </Link>
                      <ul className="mt-2 space-y-1.5">
                        {l.items.map(({ annotation: a }) => (
                          <li key={a.id} className="flex gap-2 text-sm">
                            <span
                              className="mt-1.5 size-2 shrink-0 rounded-full"
                              style={{ background: TYPE_META[a.type].color }}
                            />
                            {anchorLabel(a) ? (
                              <Link
                                to="/lessons/$lessonId"
                                params={{ lessonId: a.lessonId }}
                                search={anchorSearch(a)}
                                className="shrink-0 font-mono text-xs leading-6 text-primary hover:underline"
                              >
                                {anchorLabel(a)}
                              </Link>
                            ) : (
                              <span className="w-8 shrink-0" />
                            )}
                            <div className="min-w-0">
                              <span className="text-xs text-muted-foreground">
                                {TYPE_META[a.type].label}
                                {a.status && a.type !== "note" ? ` · ${a.status.replace("_", " ")}` : ""}
                              </span>
                              <p className="line-clamp-3 whitespace-pre-wrap">
                                {a.title ?? a.bodyMd ?? <em className="text-muted-foreground">(empty)</em>}
                              </p>
                            </div>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </section>
      <section className="min-w-0">
        <h3 className="mb-2 text-sm font-semibold">Course notebook</h3>
        <div className="h-[60vh] rounded-xl border bg-card">
          <NotebookEditor
            scope="course"
            scopeId={courseId}
            placeholder="Summaries, connections between lectures, exam prep…"
          />
        </div>
      </section>
    </div>
  );
}
