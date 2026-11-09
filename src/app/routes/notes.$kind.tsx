import { useMemo, useState } from "react";
import { Link, createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { BookMarked, ListTodo, MessageCircleQuestion } from "lucide-react";
import type { AnnotationType } from "@/bindings";
import { commands } from "@/lib/api";
import { Badge, EmptyState, Spinner } from "@/components/ui/misc";
import { Input } from "@/components/ui/input";
import { plainText } from "@/features/annotations/timeline";
import { todayYmd } from "@/lib/dates";
import { cn } from "@/lib/utils";
import { anchorLabel, anchorSearch } from "@/lib/anchors";

const KINDS = {
  questions: {
    type: "question",
    title: "Questions",
    icon: MessageCircleQuestion,
    done: "answered",
    statuses: ["open", "researching", "answered"],
  },
  todos: {
    type: "todo",
    title: "To-dos",
    icon: ListTodo,
    done: "done",
    statuses: ["open", "in_progress", "done"],
  },
  references: { type: "reference", title: "References", icon: BookMarked, done: null, statuses: [] },
} as const;
type Kind = keyof typeof KINDS;

export const Route = createFileRoute("/notes/$kind")({
  parseParams: (p) => ({ kind: (p.kind in KINDS ? p.kind : "questions") as Kind }),
  component: NotesList,
});

function NotesList() {
  const { kind } = Route.useParams();
  const meta = KINDS[kind];
  const { data, isLoading } = useQuery({
    queryKey: ["annotationsByType", meta.type],
    queryFn: () => commands.listAnnotationsByType(meta.type as AnnotationType),
  });
  const [filter, setFilter] = useState<string>("open");
  const [q, setQ] = useState("");
  const today = todayYmd();
  const items = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return (data ?? []).filter((x) => {
      const status = x.annotation.status ?? "open";
      if (meta.done && filter === "open" && status === meta.done) return false;
      if (meta.done && filter === "done" && status !== meta.done) return false;
      if (!needle) return true;
      return [x.annotation.title, x.annotation.bodyMd, x.lessonTitle, x.courseTitle].some((s) =>
        s?.toLowerCase().includes(needle),
      );
    });
  }, [data, filter, q, meta.done]);

  return (
    <div className="mx-auto max-w-4xl p-8">
      <header className="mb-6 flex flex-wrap items-center gap-3">
        <h1 className="mr-auto text-2xl font-semibold">{meta.title}</h1>
        <nav className="flex gap-1 rounded-lg bg-muted p-1" aria-label="Lists">
          {(Object.keys(KINDS) as Kind[]).map((k) => (
            <Link
              key={k}
              to="/notes/$kind"
              params={{ kind: k }}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium text-muted-foreground",
                k === kind && "bg-background text-foreground shadow-sm",
              )}
            >
              {KINDS[k].title}
            </Link>
          ))}
        </nav>
      </header>
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <Input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder={`Filter ${meta.title.toLowerCase()}…`}
          className="h-8 max-w-xs"
        />
        {meta.done && (
          <div role="radiogroup" aria-label="Status" className="flex gap-1 rounded-lg bg-muted p-1">
            {[
              ["open", "Open"],
              ["done", meta.done === "answered" ? "Answered" : "Done"],
              ["all", "All"],
            ].map(([v, label]) => (
              <button
                key={v}
                role="radio"
                aria-checked={filter === v}
                onClick={() => setFilter(v!)}
                className={cn(
                  "rounded-md px-2.5 py-0.5 text-xs text-muted-foreground",
                  filter === v && "bg-background text-foreground shadow-sm",
                )}
              >
                {label}
              </button>
            ))}
          </div>
        )}
        <span className="ml-auto text-xs text-muted-foreground">{items.length} shown</span>
      </div>
      {isLoading ? (
        <Spinner />
      ) : items.length === 0 ? (
        <EmptyState icon={<meta.icon className="size-8" />} title={`No ${meta.title.toLowerCase()} here`}>
          Add them while studying with the{" "}
          {meta.type === "todo" ? "T" : meta.type === "question" ? "Q" : "Notes panel"} key.
        </EmptyState>
      ) : (
        <ul className="divide-y rounded-xl border bg-card">
          {items.map(({ annotation: a, lessonTitle, courseTitle }) => {
            const overdue = a.type === "todo" && a.dueDate && a.dueDate < today && a.status !== "done";
            return (
              <li key={a.id} className="px-4 py-3">
                <Link
                  to="/lessons/$lessonId"
                  params={{ lessonId: a.lessonId }}
                  search={anchorSearch(a)}
                  className="block font-medium hover:underline"
                >
                  {a.title || plainText(a.bodyMd, 140) || <em className="text-muted-foreground">Untitled</em>}
                </Link>
                {a.title && a.bodyMd && (
                  <p className="mt-0.5 line-clamp-2 text-sm text-muted-foreground">
                    {plainText(a.bodyMd, 240)}
                  </p>
                )}
                {a.type === "reference" && a.url && (
                  <p className="mt-0.5 truncate text-xs text-primary">{a.url}</p>
                )}
                <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                  {a.status && meta.done && (
                    <Badge tone={a.status === meta.done ? "success" : "muted"} className="capitalize">
                      {a.status.replace("_", " ")}
                    </Badge>
                  )}
                  {a.dueDate && <span className={cn(overdue && "text-warning")}>due {a.dueDate}</span>}
                  <span className="truncate">
                    {courseTitle} · {lessonTitle}
                    {anchorLabel(a) ? ` @ ${anchorLabel(a)}` : ""}
                  </span>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
