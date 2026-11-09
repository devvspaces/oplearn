import { useEffect, useMemo, useState } from "react";
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCorners,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragOverEvent,
} from "@dnd-kit/core";
import { SortableContext, sortableKeyboardCoordinates, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronUp, FolderPlus, MoreHorizontal, Plus } from "lucide-react";
import type { CourseDetail, Lesson, LessonType, PlannedLesson } from "@/bindings";
import { commands, useLibraryMutation } from "@/lib/api";
import { formatDuration } from "@/lib/time";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { LessonRow } from "./LessonRow";
import { useDownloadsList } from "@/features/downloads/useDownloads";
import { useLessonDownload } from "@/features/downloads/useLessonDownload";
import { errorMessage } from "@/lib/api";
import { toast } from "sonner";
import { PromptDialog, type PromptField } from "./PromptDialog";
import { findContainer, moveLesson, sameLayout, type Layout } from "./layout";
import { cn } from "@/lib/utils";

interface PromptState {
  title: string;
  description?: string;
  fields: PromptField[];
  submitLabel?: string;
  onSubmit: (v: Record<string, string>) => Promise<unknown>;
}

function toLayout(d: CourseDetail): Layout {
  return d.modules.map((m) => ({ moduleId: m.module.id, lessonIds: m.lessons.map((l) => l.id) }));
}

export function CourseOutline({ detail, plan }: { detail: CourseDetail; plan?: PlannedLesson[] }) {
  const plannedByLesson = useMemo(
    () => new Map((plan ?? []).map((p) => [p.lessonId, p.plannedDate])),
    [plan],
  );
  const courseId = detail.course.id;
  const serverLayout = useMemo(() => toLayout(detail), [detail]);
  const [layout, setLayout] = useState<Layout>(serverLayout);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [prompt, setPrompt] = useState<PromptState | null>(null);
  useEffect(() => setLayout(serverLayout), [serverLayout]);

  const lessons = useMemo(() => {
    const m = new Map<string, Lesson>();
    detail.modules.forEach((mod) => mod.lessons.forEach((l) => m.set(l.id, l)));
    return m;
  }, [detail]);

  const saveLayout = useLibraryMutation((l: Layout) => commands.applyLayout(courseId, l));
  const reorderModules = useLibraryMutation((ids: string[]) => commands.reorderModules(courseId, ids));
  const toggle = useLibraryMutation((l: Lesson) => commands.setLessonCompleted(l.id, l.completedAt == null));
  const { data: downloadList } = useDownloadsList();
  const downloadsByLesson = useMemo(
    () => new Map((downloadList ?? []).map((d) => [d.lessonId, d])),
    [downloadList],
  );
  const run = (fn: () => Promise<unknown>, ok?: string) =>
    fn().then(
      () => ok && toast.success(ok),
      (e: unknown) => toast.error(errorMessage(e)),
    );
  const downloadLesson = useLessonDownload();
  const del = useLibraryMutation(commands.deleteLesson, { success: "Lesson deleted" });
  const delModule = useLibraryMutation(commands.deleteModule, { success: "Module deleted" });
  const mutate = useLibraryMutation(async (fn: () => Promise<unknown>) => fn());

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  function onDragOver({ active, over }: DragOverEvent) {
    if (!over) return;
    const a = String(active.id);
    const o = String(over.id);
    // Cross-module moves happen live while dragging; same-module reorders on drop.
    if (findContainer(layout, a) !== findContainer(layout, o)) setLayout((l) => moveLesson(l, a, o));
  }

  function onDragEnd({ active, over }: DragEndEvent) {
    const next = over ? moveLesson(layout, String(active.id), String(over.id)) : layout;
    setLayout(next);
    if (!sameLayout(next, serverLayout)) saveLayout.mutate(next);
  }

  const openPrompt = (p: PromptState) => setPrompt(p);

  const addModule = () =>
    openPrompt({
      title: "New module",
      description: "E.g. “Week 1: MapReduce & RPC”. Drag lessons into it afterwards.",
      fields: [{ name: "title", label: "Module title" }],
      submitLabel: "Add module",
      onSubmit: (v) => mutate.mutateAsync(() => commands.createModule(courseId, v.title!)),
    });

  const addVideo = (moduleId: string) =>
    openPrompt({
      title: "Add YouTube video",
      fields: [{ name: "url", label: "Video URL", placeholder: "https://www.youtube.com/watch?v=…" }],
      submitLabel: "Add video",
      onSubmit: (v) =>
        commands.addVideoToModule(moduleId, v.url!).then(() => mutate.mutateAsync(async () => null)),
    });

  const addOther = (moduleId: string, type: LessonType, label: string) =>
    openPrompt({
      title: `Add ${label}`,
      fields: [
        { name: "title", label: "Title" },
        {
          name: "url",
          label: type === "reading" ? "Link (URL)" : "Link (optional)",
          required: type === "reading",
        },
      ],
      submitLabel: "Add",
      onSubmit: (v) =>
        mutate.mutateAsync(() =>
          commands.createLesson({ moduleId, type, title: v.title!, url: v.url?.trim() || null }),
        ),
    });

  const moveModule = (index: number, dir: -1 | 1) => {
    const ids = layout.map((m) => m.moduleId);
    const j = index + dir;
    if (j < 0 || j >= ids.length) return;
    [ids[index], ids[j]] = [ids[j]!, ids[index]!];
    reorderModules.mutate(ids);
  };

  return (
    <div className="space-y-3">
      <DndContext
        sensors={sensors}
        collisionDetection={closestCorners}
        onDragOver={onDragOver}
        onDragEnd={onDragEnd}
      >
        {layout.map((mo, mi) => {
          const mod = detail.modules.find((m) => m.module.id === mo.moduleId)?.module;
          if (!mod) return null;
          const items = mo.lessonIds.map((id) => lessons.get(id)).filter((l): l is Lesson => !!l);
          const done = items.filter((l) => l.completedAt).length;
          const dur = items.reduce((s, l) => s + (l.durationSec ?? 0), 0);
          const offset = layout.slice(0, mi).reduce((s, m) => s + m.lessonIds.length, 0);
          const isCollapsed = collapsed[mod.id];
          return (
            <section key={mod.id} className="rounded-xl border bg-card" aria-labelledby={`mod-${mod.id}`}>
              <header className="flex items-center gap-2 px-3 py-2.5">
                <button
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                  onClick={() => setCollapsed((c) => ({ ...c, [mod.id]: !c[mod.id] }))}
                  aria-expanded={!isCollapsed}
                >
                  <ChevronDown
                    className={cn("size-4 shrink-0 transition-transform", isCollapsed && "-rotate-90")}
                  />
                  <h3 id={`mod-${mod.id}`} className="truncate text-sm font-semibold">
                    {mod.title}
                  </h3>
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {done}/{items.length} · {formatDuration(dur) || "0m"}
                  </span>
                </button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="icon-sm" aria-label={`Add to ${mod.title}`}>
                      <Plus />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem onSelect={() => addVideo(mod.id)}>YouTube video…</DropdownMenuItem>
                    <DropdownMenuItem onSelect={() => addOther(mod.id, "reading", "reading")}>
                      Reading (link)…
                    </DropdownMenuItem>
                    <DropdownMenuItem onSelect={() => addOther(mod.id, "lab", "lab / assignment")}>
                      Lab / assignment…
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="icon-sm" aria-label={`Module actions for ${mod.title}`}>
                      <MoreHorizontal />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem
                      onSelect={() =>
                        openPrompt({
                          title: "Rename module",
                          fields: [{ name: "title", label: "Module title", initial: mod.title }],
                          onSubmit: (v) => mutate.mutateAsync(() => commands.renameModule(mod.id, v.title!)),
                        })
                      }
                    >
                      Rename…
                    </DropdownMenuItem>
                    <DropdownMenuItem disabled={mi === 0} onSelect={() => moveModule(mi, -1)}>
                      <ChevronUp /> Move up
                    </DropdownMenuItem>
                    <DropdownMenuItem disabled={mi === layout.length - 1} onSelect={() => moveModule(mi, 1)}>
                      <ChevronDown /> Move down
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      variant="destructive"
                      onSelect={async () => {
                        const msg = items.length
                          ? `Delete “${mod.title}”? Its ${items.length} lessons move to the neighbouring module.`
                          : `Delete “${mod.title}”?`;
                        if (await ask(msg, { title: "Delete module", kind: "warning" }))
                          delModule.mutate(mod.id);
                      }}
                    >
                      Delete module
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </header>
              {!isCollapsed && (
                <ModuleDropZone id={mod.id} empty={items.length === 0}>
                  <SortableContext items={mo.lessonIds} strategy={verticalListSortingStrategy}>
                    {items.map((l, i) => (
                      <LessonRow
                        key={l.id}
                        lesson={l}
                        index={offset + i}
                        onToggleComplete={() => toggle.mutate(l)}
                        download={downloadsByLesson.get(l.id)}
                        onDownload={(q) => void downloadLesson(l.id, q)}
                        plannedDate={plannedByLesson.get(l.id)}
                        onCancelDownload={() => {
                          const d = downloadsByLesson.get(l.id);
                          if (d) void run(() => commands.cancelDownload(d.id));
                        }}
                        onRemoveDownload={async () => {
                          if (
                            await ask(
                              `Delete the downloaded video for “${l.title}”? Notes and progress are kept.`,
                              { kind: "warning" },
                            )
                          )
                            void run(() => commands.deleteLocalFiles(courseId, l.id), "Download removed");
                        }}
                        onRename={() =>
                          openPrompt({
                            title: "Rename lesson",
                            fields: [{ name: "title", label: "Title", initial: l.title }],
                            onSubmit: (v) =>
                              mutate.mutateAsync(() => commands.updateLesson(l.id, { title: v.title! })),
                          })
                        }
                        onDelete={async () => {
                          if (
                            await ask(`Delete “${l.title}”? Its progress will be lost.`, {
                              title: "Delete lesson",
                              kind: "warning",
                            })
                          )
                            del.mutate(l.id);
                        }}
                      />
                    ))}
                  </SortableContext>
                </ModuleDropZone>
              )}
            </section>
          );
        })}
      </DndContext>
      <Button variant="outline" onClick={addModule}>
        <FolderPlus /> Add module
      </Button>
      {prompt && <PromptDialog open onOpenChange={(o) => !o && setPrompt(null)} {...prompt} />}
    </div>
  );
}

function ModuleDropZone({ id, empty, children }: { id: string; empty: boolean; children: React.ReactNode }) {
  const { setNodeRef, isOver } = useDroppable({ id });
  return (
    <ul ref={setNodeRef} className={cn("border-t px-1 py-1", isOver && "bg-primary/5")}>
      {children}
      {empty && (
        <li className="px-3 py-3 text-xs text-muted-foreground">Drag lessons here, or use + to add one.</li>
      )}
    </ul>
  );
}
