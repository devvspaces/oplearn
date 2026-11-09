import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, MoreHorizontal, Search, Sparkles, Trash2, Layers } from "lucide-react";
import type { Annotation, AnnotationUpdate } from "@/bindings";
import { commands, errorMessage, qk } from "@/lib/api";
import { useAutosave } from "@/lib/autosave";
import { formatTimestamp } from "@/lib/time";
import { MarkdownEditor } from "@/components/MarkdownEditor";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/misc";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { player } from "@/features/player/store";
import { pdfNav } from "@/features/materials/pdf-nav";
import { plainText, TYPE_META } from "./timeline";
import { useCardDialog } from "@/features/flashcards/api";
import { AssistDialog } from "@/features/ai/AssistDialog";
import { cn } from "@/lib/utils";
import { toast } from "sonner";

const selectCls = "rounded-md border bg-background px-1.5 py-1 text-xs";

export function AnnotationItem({
  a,
  courseTitle,
  near,
  autoFocus,
  onDelete,
}: {
  a: Annotation;
  courseTitle: string;
  near: boolean;
  autoFocus: boolean;
  onDelete: () => void;
}) {
  const qc = useQueryClient();
  const meta = TYPE_META[a.type];
  // Edits patch the cached list directly (no refetch per keystroke) and persist debounced.
  const patchCache = (u: AnnotationUpdate) =>
    qc.setQueryData<Annotation[]>(qk.annotations(a.lessonId), (list) =>
      list?.map((x) => (x.id === a.id ? ({ ...x, ...u } as Annotation) : x)),
    );
  const { schedule, flush } = useAutosave<AnnotationUpdate>((u) =>
    commands.updateAnnotation(a.id, u).then(
      (saved) => {
        qc.setQueryData<Annotation[]>(qk.annotations(a.lessonId), (list) =>
          list?.map((x) => (x.id === a.id ? saved : x)),
        );
        void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
      },
      (e: unknown) => toast.error(errorMessage(e)),
    ),
  );
  // Accumulate field edits so a debounced save carries all of them.
  const [draft, setDraft] = useState<AnnotationUpdate>({});
  const edit = (u: AnnotationUpdate) => {
    const next = { ...draft, ...u };
    setDraft(next);
    patchCache(u);
    schedule(next);
  };
  const editNow = (u: AnnotationUpdate) => {
    edit(u);
    void Promise.resolve().then(flush);
  };

  const question = a.type === "question";
  const [askOpen, setAskOpen] = useState(false);
  const todo = a.type === "todo";
  const reference = a.type === "reference";
  const done = todo && a.status === "done";

  return (
    <li
      className={cn(
        "group rounded-lg border bg-card/60 p-2.5 transition-colors",
        near && "border-primary/50 bg-primary/5",
      )}
      data-annotation-id={a.id}
    >
      <div className="flex items-center gap-2">
        <span className="size-2 shrink-0 rounded-full" style={{ background: meta.color }} aria-hidden />
        {todo && (
          <input
            type="checkbox"
            checked={done}
            onChange={(e) => editNow({ status: e.target.checked ? "done" : "open" })}
            aria-label={done ? "Mark to-do open" : "Mark to-do done"}
            className="accent-[var(--color-todo)]"
          />
        )}
        <span className="text-xs font-medium">{meta.label}</span>
        {a.startSec != null ? (
          <button
            onClick={() => player.seek(a.startSec!)}
            className="rounded px-1 font-mono text-[11px] text-primary hover:bg-primary/10"
            title="Jump to this moment"
          >
            {formatTimestamp(a.startSec)}
          </button>
        ) : a.page != null && a.materialId ? (
          <button
            onClick={() => pdfNav.goTo(a.materialId!, a.page!)}
            className="rounded px-1 font-mono text-[11px] text-primary hover:bg-primary/10"
            title="Go to this page"
          >
            p.{a.page}
          </button>
        ) : null}
        {question && (
          <select
            aria-label="Question status"
            value={a.status ?? "open"}
            onChange={(e) => editNow({ status: e.target.value })}
            className={selectCls}
          >
            <option value="open">Open</option>
            <option value="researching">Researching</option>
            <option value="answered">Answered</option>
          </select>
        )}
        {a.aiGenerated && <Badge tone="primary">AI</Badge>}
        <div className="flex-1" />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`${meta.label} actions`}
              className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
            >
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {a.materialId ? (
              <DropdownMenuItem
                onSelect={() => {
                  const cur = pdfNav.current();
                  if (cur?.materialId === a.materialId) editNow({ page: cur.page });
                }}
              >
                Move to the current page
              </DropdownMenuItem>
            ) : (
              <DropdownMenuItem onSelect={() => editNow({ startSec: player.time() })}>
                Set time to now
              </DropdownMenuItem>
            )}
            <DropdownMenuItem
              onSelect={() => {
                const body = plainText(a.bodyMd ?? "", 2000);
                const lines = body.split(/(?<=[.?!])\s+|\n+/).filter(Boolean);
                useCardDialog.getState().open({
                  lessonId: a.lessonId,
                  sourceSec: a.startSec,
                  sourceMaterialId: a.materialId,
                  sourcePage: a.page,
                  front: a.title?.trim() || (a.type === "question" ? body : (lines[0] ?? "")),
                  back:
                    a.type === "question"
                      ? plainText(a.answerMd ?? "", 2000)
                      : a.title?.trim()
                        ? body
                        : lines.slice(1).join(" "),
                });
              }}
            >
              <Layers /> Make flashcard…
            </DropdownMenuItem>
            <DropdownMenuItem variant="destructive" onSelect={onDelete}>
              <Trash2 /> Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {a.type === "bookmark" && (
        <Input
          className="mt-2 h-8 text-sm"
          placeholder="Label (optional)"
          defaultValue={a.title ?? ""}
          autoFocus={autoFocus}
          onChange={(e) => edit({ title: e.target.value })}
          onKeyDown={(e) => e.key === "Escape" && e.currentTarget.blur()}
        />
      )}

      {reference && (
        <div className="mt-2 grid gap-1.5">
          <Input
            className="h-8 text-sm"
            placeholder="Title"
            defaultValue={a.title ?? ""}
            autoFocus={autoFocus}
            onChange={(e) => edit({ title: e.target.value })}
          />
          <div className="flex gap-1.5">
            <Input
              className="h-8 text-sm"
              placeholder="https://…"
              defaultValue={a.url ?? ""}
              onChange={(e) => edit({ url: e.target.value })}
            />
            <select
              aria-label="Reference type"
              value={a.refType ?? ""}
              onChange={(e) => editNow({ refType: e.target.value })}
              className={selectCls}
            >
              <option value="">Type…</option>
              {["paper", "article", "repo", "book", "video", "other"].map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
            {a.url && (
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Open link"
                onClick={() => void openUrl(a.url!)}
              >
                <ExternalLink />
              </Button>
            )}
          </div>
        </div>
      )}

      {a.type !== "bookmark" && (
        <MarkdownEditor
          className={cn("mt-1.5 rounded-md px-1", done && "line-through opacity-60")}
          value={a.bodyMd ?? ""}
          autoFocus={autoFocus && !reference}
          ariaLabel={`${meta.label} text`}
          placeholder={
            question
              ? "What don't you understand yet?"
              : todo
                ? "What needs doing?"
                : reference
                  ? "Citation / why it matters"
                  : "Write a note…"
          }
          onChange={(md) => edit({ bodyMd: md })}
        />
      )}

      {todo && (
        <div className="mt-1.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <label className="flex items-center gap-1">
            Priority
            <select
              value={a.priority ?? ""}
              onChange={(e) => editNow({ priority: e.target.value })}
              className={selectCls}
            >
              <option value="">—</option>
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
          </label>
          <label className="flex items-center gap-1">
            Due
            <input
              type="date"
              value={a.dueDate ?? ""}
              onChange={(e) => editNow({ dueDate: e.target.value })}
              className={selectCls}
            />
          </label>
        </div>
      )}

      {question && (
        <div className="mt-2 border-t pt-2">
          <div className="mb-1 flex items-center gap-1">
            <span className="text-xs font-medium text-muted-foreground">Answer</span>
            <div className="flex-1" />
            <Button
              variant="ghost"
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={() => {
                const q = `${(a.bodyMd ?? "").replace(/[#*_`>]/g, " ").trim()} ${courseTitle}`.trim();
                void openUrl(`https://duckduckgo.com/?q=${encodeURIComponent(q)}`);
              }}
              title="Search the web for this question"
            >
              <Search /> Research
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={() => setAskOpen(true)}
              title="Answer this question with AI, using the lesson around it"
            >
              <Sparkles /> Ask AI
            </Button>
            <AssistDialog
              open={askOpen}
              onOpenChange={setAskOpen}
              kind="answer"
              focus={{
                lessonId: a.lessonId,
                sec: a.startSec ?? undefined,
                materialId: a.materialId ?? undefined,
                page: a.page ?? undefined,
              }}
              question={[a.title, plainText(a.bodyMd ?? "", 1500)].filter(Boolean).join(" — ")}
              description="Claude answers from the lecture around this question and cites where. The answer is saved here, marked AI-generated."
              saveLabel="Save answer"
              onSave={async (ans) => {
                const saved = await commands.saveAiAnswer(a.id, ans.markdown);
                qc.setQueryData<Annotation[]>(qk.annotations(a.lessonId), (list) =>
                  list?.map((x) => (x.id === a.id ? saved : x)),
                );
                void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
                toast.success("Answer saved");
              }}
            />
          </div>
          <MarkdownEditor
            className="rounded-md px-1"
            value={a.answerMd ?? ""}
            ariaLabel="Answer"
            placeholder="Write the answer when you find it…"
            onChange={(md) => edit({ answerMd: md })}
          />
        </div>
      )}
    </li>
  );
}
