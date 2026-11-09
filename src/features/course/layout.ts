/** Pure helpers for the drag-and-drop outline (unit-tested in tests/unit/layout.test.ts). */
import type { ModuleOrder } from "@/bindings";

export type Layout = ModuleOrder[];

export function findContainer(layout: Layout, id: string): string | undefined {
  if (layout.some((m) => m.moduleId === id)) return id;
  return layout.find((m) => m.lessonIds.includes(id))?.moduleId;
}

/**
 * Moves `activeId` to the position of `overId` (a lesson or a module container).
 * Returns the same object if nothing changes.
 */
export function moveLesson(layout: Layout, activeId: string, overId: string): Layout {
  const from = findContainer(layout, activeId);
  const to = findContainer(layout, overId);
  if (!from || !to || activeId === overId) return layout;
  const next = layout.map((m) => ({ ...m, lessonIds: [...m.lessonIds] }));
  const src = next.find((m) => m.moduleId === from)!;
  const dst = next.find((m) => m.moduleId === to)!;
  const fromIdx = src.lessonIds.indexOf(activeId);
  src.lessonIds.splice(fromIdx, 1);
  let toIdx = overId === to ? dst.lessonIds.length : dst.lessonIds.indexOf(overId);
  if (toIdx < 0) toIdx = dst.lessonIds.length;
  // Dragging downwards within the same list lands after the target.
  if (from === to && fromIdx <= toIdx && overId !== to) toIdx += 1;
  if (from === to && fromIdx === toIdx) return layout;
  dst.lessonIds.splice(Math.min(toIdx, dst.lessonIds.length), 0, activeId);
  return next;
}

export function sameLayout(a: Layout, b: Layout): boolean {
  return (
    a.length === b.length &&
    a.every((m, i) => m.moduleId === b[i]!.moduleId && m.lessonIds.join() === b[i]!.lessonIds.join())
  );
}
