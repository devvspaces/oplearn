import { useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { BookMarked, Plus } from "lucide-react";
import type { AnnotationType } from "@/bindings";
import { commands, useAnnotationMutation, useAnnotations } from "@/lib/api";
import { usePlayer } from "@/features/player/store";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ReferencesDialog } from "@/features/ai/ReferencesDialog";
import { Spinner } from "@/components/ui/misc";
import { AnnotationItem } from "./AnnotationItem";
import { TYPE_META, isNear } from "./timeline";
import { cn } from "@/lib/utils";

export type AnnotationFilter = "all" | AnnotationType;
const FILTERS: AnnotationFilter[] = ["all", "note", "bookmark", "question", "todo", "reference"];

export function AnnotationsPanel({
  lessonId,
  courseTitle,
  focusId,
  onCreate,
}: {
  lessonId: string;
  courseTitle: string;
  /** Newly created item to scroll to and focus. */
  focusId: string | null;
  onCreate: (type: AnnotationType) => void;
}) {
  const [refsOpen, setRefsOpen] = useState(false);
  const { data, isLoading } = useAnnotations(lessonId);
  const [filter, setFilter] = useState<AnnotationFilter>("all");
  const time = usePlayer((s) => s.time);
  const listRef = useRef<HTMLUListElement>(null);
  const del = useAnnotationMutation(lessonId, commands.deleteAnnotation);

  const items = useMemo(
    () => (data ?? []).filter((a) => filter === "all" || a.type === filter),
    [data, filter],
  );
  const counts = useMemo(() => {
    const c: Record<string, number> = { all: data?.length ?? 0 };
    data?.forEach((a) => (c[a.type] = (c[a.type] ?? 0) + 1));
    return c;
  }, [data]);

  useEffect(() => {
    if (!focusId) return;
    setFilter((f) => (f === "all" ? f : "all"));
    requestAnimationFrame(() =>
      listRef.current
        ?.querySelector(`[data-annotation-id="${focusId}"]`)
        ?.scrollIntoView({ block: "nearest" }),
    );
  }, [focusId, data]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-1 border-b px-2 py-1.5">
        <div
          role="tablist"
          aria-label="Filter annotations"
          className="flex min-w-0 flex-1 gap-0.5 overflow-x-auto"
        >
          {FILTERS.map((f) => (
            <button
              key={f}
              role="tab"
              aria-selected={filter === f}
              onClick={() => setFilter(f)}
              className={cn(
                "shrink-0 rounded px-2 py-1 text-[11px] font-medium text-muted-foreground hover:text-foreground",
                filter === f && "bg-accent text-foreground",
              )}
            >
              {f === "all" ? "All" : TYPE_META[f].plural}
              {counts[f] ? <span className="ml-1 opacity-60">{counts[f]}</span> : null}
            </button>
          ))}
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" aria-label="Add annotation at current time">
              <Plus />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {(Object.keys(TYPE_META) as AnnotationType[]).map((t) => (
              <DropdownMenuItem key={t} onSelect={() => onCreate(t)}>
                <span className="size-2 rounded-full" style={{ background: TYPE_META[t].color }} />
                {TYPE_META[t].label}
                {TYPE_META[t].key && (
                  <kbd className="ml-auto font-mono text-[10px] text-muted-foreground">
                    {TYPE_META[t].key}
                  </kbd>
                )}
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => setRefsOpen(true)}>
              <BookMarked className="size-3.5" /> Suggest references with AI…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <ReferencesDialog lessonId={lessonId} open={refsOpen} onOpenChange={setRefsOpen} />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {isLoading ? (
          <Spinner />
        ) : items.length === 0 ? (
          <div className="px-2 py-8 text-center text-xs text-muted-foreground">
            <p>Nothing here yet.</p>
            <p className="mt-1">
              Press <kbd className="rounded border px-1 font-mono">N</kbd> note ·{" "}
              <kbd className="rounded border px-1 font-mono">B</kbd> bookmark ·{" "}
              <kbd className="rounded border px-1 font-mono">Q</kbd> question ·{" "}
              <kbd className="rounded border px-1 font-mono">T</kbd> to-do
            </p>
          </div>
        ) : (
          <ul ref={listRef} className="space-y-2">
            {items.map((a) => (
              <AnnotationItem
                key={a.id}
                a={a}
                courseTitle={courseTitle}
                near={isNear(a, time)}
                autoFocus={a.id === focusId}
                onDelete={async () => {
                  const empty = !a.bodyMd && !a.title && !a.answerMd;
                  if (
                    empty ||
                    (await ask(`Delete this ${TYPE_META[a.type].label.toLowerCase()}?`, { kind: "warning" }))
                  )
                    del.mutate(a.id);
                }}
              />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
