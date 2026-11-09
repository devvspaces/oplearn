import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { Dialog as D } from "radix-ui";
import { BookOpen, Captions, FileText, Layers, NotebookPen, PlayCircle, Search } from "lucide-react";
import type { SearchHit, SearchResults } from "@/bindings";
import { commands } from "@/lib/api";
import { formatTimestamp } from "@/lib/time";
import { TYPE_META } from "@/features/annotations/timeline";
import { Spinner } from "@/components/ui/misc";
import { cn } from "@/lib/utils";

const GROUPS: Array<{ key: keyof SearchResults; label: string; icon: React.ElementType }> = [
  { key: "transcripts", label: "Spoken in lectures", icon: Captions },
  { key: "annotations", label: "Your notes", icon: NotebookPen },
  { key: "lessons", label: "Lessons", icon: PlayCircle },
  { key: "notebooks", label: "Notebooks", icon: BookOpen },
  { key: "flashcards", label: "Flashcards", icon: Layers },
  { key: "materials", label: "Course materials (PDF)", icon: FileText },
];

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

/** Flattens grouped results into a single keyboard-navigable list. */
export function flattenResults(
  r: SearchResults | undefined,
): Array<{ group: keyof SearchResults; hit: SearchHit }> {
  if (!r) return [];
  return GROUPS.flatMap((g) => (r[g.key] ?? []).map((hit) => ({ group: g.key, hit })));
}

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const q = useDebounced(query.trim(), 120);
  const navigate = useNavigate();
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const { data, isFetching } = useQuery({
    queryKey: ["search", q],
    queryFn: () => commands.searchEverything(q, 8),
    enabled: open && q.length > 0,
    staleTime: 5_000,
    placeholderData: (prev) => prev,
  });
  const flat = useMemo(() => (q ? flattenResults(data) : []), [data, q]);
  useEffect(() => setActive(0), [q]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const go = (hit: SearchHit, group?: keyof SearchResults) => {
    setOpen(false);
    if (group === "materials" && hit.itemId) {
      // Open the hit's page with the words highlighted: inside its lesson when it has one
      // (notes and progress), otherwise in the document view.
      const search = { page: hit.page ?? undefined, q: q || undefined };
      if (hit.lessonId)
        void navigate({
          to: "/lessons/$lessonId",
          params: { lessonId: hit.lessonId },
          search: { ...search, doc: hit.itemId },
        });
      else void navigate({ to: "/materials/$materialId", params: { materialId: hit.itemId }, search });
    } else if (group === "flashcards" && !hit.lessonId) {
      void navigate({ to: "/review" });
    } else if (hit.lessonId) {
      void navigate({
        to: "/lessons/$lessonId",
        params: { lessonId: hit.lessonId },
        search: hit.startSec != null ? { t: Math.floor(hit.startSec) } : {},
      });
    } else {
      void navigate({ to: "/courses/$courseId", params: { courseId: hit.courseId } });
    }
  };

  let index = -1;
  return (
    <D.Root
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setQuery("");
      }}
    >
      <D.Portal>
        <D.Overlay className="fixed inset-0 z-50 bg-black/50" />
        <D.Content
          aria-describedby={undefined}
          className="fixed left-1/2 top-[12vh] z-50 flex max-h-[70vh] w-full max-w-2xl -translate-x-1/2 flex-col overflow-hidden rounded-xl border bg-popover shadow-2xl"
        >
          <D.Title className="sr-only">Search everything</D.Title>
          <div className="flex items-center gap-2 border-b px-4">
            <Search className="size-4 text-muted-foreground" aria-hidden />
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setActive((a) => Math.min(flat.length - 1, a + 1));
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setActive((a) => Math.max(0, a - 1));
                } else if (e.key === "Enter" && flat[active]) {
                  e.preventDefault();
                  go(flat[active].hit, flat[active].group);
                }
              }}
              placeholder="Search lectures, notes, lessons…"
              aria-label="Search everything"
              role="combobox"
              aria-expanded={flat.length > 0}
              aria-controls="search-results"
              className="h-12 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            />
            {isFetching && <Spinner className="size-3.5 text-muted-foreground" />}
            <kbd className="rounded border px-1.5 font-mono text-[10px] text-muted-foreground">esc</kbd>
          </div>
          <div
            ref={listRef}
            id="search-results"
            role="listbox"
            className="min-h-0 flex-1 overflow-y-auto p-2"
          >
            {!q ? (
              <p className="px-3 py-6 text-center text-xs text-muted-foreground">
                Search words spoken in your lectures, your notes and questions, lesson titles and notebooks.
              </p>
            ) : flat.length === 0 && !isFetching ? (
              <p className="px-3 py-6 text-center text-xs text-muted-foreground">No matches for “{q}”.</p>
            ) : (
              GROUPS.map(({ key, label, icon: Icon }) => {
                const hits = data?.[key] ?? [];
                if (!hits.length) return null;
                return (
                  <div key={key} className="mb-2" role="group" aria-label={label}>
                    <div className="px-2 py-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
                      {label}
                    </div>
                    {hits.map((hit) => {
                      index += 1;
                      const i = index;
                      return (
                        <button
                          key={`${key}-${i}`}
                          data-index={i}
                          role="option"
                          aria-selected={i === active}
                          onMouseMove={() => setActive(i)}
                          onClick={() => go(hit, key)}
                          className={cn(
                            "flex w-full gap-3 rounded-md px-2 py-2 text-left",
                            i === active && "bg-accent",
                          )}
                        >
                          {hit.annotationType ? (
                            <span
                              className="mt-1.5 size-2 shrink-0 rounded-full"
                              style={{ background: TYPE_META[hit.annotationType].color }}
                            />
                          ) : (
                            <Icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
                          )}
                          <span className="min-w-0 flex-1">
                            {/* Snippets are HTML-escaped in Rust; only <mark> tags are added. */}
                            <span
                              className="line-clamp-2 text-sm"
                              dangerouslySetInnerHTML={{ __html: hit.snippet }}
                            />
                            <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
                              {hit.courseTitle}
                              {hit.lessonTitle ? ` · ${hit.lessonTitle}` : ""}
                            </span>
                          </span>
                          {hit.page != null && (
                            <span className="shrink-0 font-mono text-[11px] text-primary">p.{hit.page}</span>
                          )}
                          {hit.startSec != null && (
                            <span className="shrink-0 font-mono text-[11px] text-primary">
                              {formatTimestamp(hit.startSec)}
                            </span>
                          )}
                        </button>
                      );
                    })}
                  </div>
                );
              })
            )}
          </div>
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}
