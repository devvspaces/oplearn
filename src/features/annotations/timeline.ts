import type { Annotation, AnnotationType } from "@/bindings";

export const TYPE_META: Record<
  AnnotationType,
  { label: string; plural: string; color: string; key: string }
> = {
  note: { label: "Note", plural: "Notes", color: "var(--color-note)", key: "N" },
  bookmark: { label: "Bookmark", plural: "Bookmarks", color: "var(--color-bookmark)", key: "B" },
  question: { label: "Question", plural: "Questions", color: "var(--color-question)", key: "Q" },
  todo: { label: "To-do", plural: "To-dos", color: "var(--color-todo)", key: "T" },
  reference: { label: "Reference", plural: "References", color: "var(--color-reference)", key: "" },
};

/** Position of a timestamp along the seek bar, in percent (clamped), or null if unknown. */
export function markerPercent(sec: number | null | undefined, duration: number): number | null {
  if (sec == null || !Number.isFinite(sec) || !(duration > 0)) return null;
  return Math.min(100, Math.max(0, (sec / duration) * 100));
}

/** Whether an annotation is "near" the playhead, for highlighting in the side panel. */
export function isNear(a: Pick<Annotation, "startSec" | "endSec">, t: number, windowSec = 15): boolean {
  if (a.startSec == null) return false;
  const end = a.endSec ?? a.startSec;
  return t >= a.startSec - windowSec && t <= end + windowSec;
}

/**
 * Collapses markers that would overlap visually (closer than `minGapPct`) into one, keeping
 * the earliest; returns `{ percent, items }` groups for rendering.
 */
export function groupMarkers<T extends { startSec: number | null }>(
  items: readonly T[],
  duration: number,
  minGapPct = 0.6,
): { percent: number; items: T[] }[] {
  const placed = items
    .map((it) => ({ it, p: markerPercent(it.startSec, duration) }))
    .filter((x): x is { it: T; p: number } => x.p != null)
    .sort((a, b) => a.p - b.p);
  const groups: { percent: number; items: T[] }[] = [];
  for (const { it, p } of placed) {
    const last = groups[groups.length - 1];
    if (last && p - last.percent < minGapPct) last.items.push(it);
    else groups.push({ percent: p, items: [it] });
  }
  return groups;
}

/** Rough Markdown → plain text for previews (hover cards, lists). */
export function plainText(md: string | null | undefined, maxLen = 160): string {
  if (!md) return "";
  const text = md
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/^\s{0,3}(#{1,6}|>|[-*+]|\d+\.)\s+/gm, "")
    .replace(/(\*\*|__|\*|_|~~)(.+?)\1/g, "$2")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > maxLen ? `${text.slice(0, maxLen - 1).trimEnd()}…` : text;
}
