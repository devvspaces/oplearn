/** Pure helpers for the chapter-segmented seek bar (unit-tested in tests/unit/segments.test.ts). */
import type { Chapter } from "@/bindings";

export interface Segment {
  start: number;
  end: number;
  title: string | null;
  leftPct: number;
  widthPct: number;
}

/**
 * Splits [0, duration] into chapter segments. Chapters may be unsorted, overlap, lack an end,
 * or extend past the duration. With no usable chapters there is one untitled segment.
 */
export function buildSegments(
  chapters: readonly Pick<Chapter, "startSec" | "endSec" | "title">[],
  duration: number,
): Segment[] {
  if (!(duration > 0)) return [];
  const starts = chapters
    .map((c) => ({ start: Math.max(0, c.startSec ?? 0), title: c.title }))
    .filter((c) => c.start < duration)
    .sort((a, b) => a.start - b.start)
    // Drop duplicates at the same start (keep the first).
    .filter((c, i, arr) => i === 0 || c.start > arr[i - 1]!.start);
  if (starts.length === 0) return [{ start: 0, end: duration, title: null, leftPct: 0, widthPct: 100 }];
  // Anything before the first chapter becomes an untitled lead-in segment.
  if (starts[0]!.start > 0) starts.unshift({ start: 0, title: "" });
  return starts.map((c, i) => {
    const end = i + 1 < starts.length ? starts[i + 1]!.start : duration;
    return {
      start: c.start,
      end,
      title: c.title || null,
      leftPct: (c.start / duration) * 100,
      widthPct: ((end - c.start) / duration) * 100,
    };
  });
}

/** The segment containing time `t` (the last one whose start ≤ t). */
export function chapterAt(segments: readonly Segment[], t: number): Segment | null {
  let found: Segment | null = null;
  for (const s of segments) {
    if (s.start <= t) found = s;
    else break;
  }
  return found ?? segments[0] ?? null;
}

/** Time under a pointer at `x` px within a bar `width` px wide, clamped to [0, duration]. */
export function timeAtPointer(x: number, width: number, duration: number): number {
  if (!(width > 0) || !(duration > 0)) return 0;
  return Math.min(duration, Math.max(0, (x / width) * duration));
}

/** How much of a segment is filled (0–1) at playhead `t`. */
export function segmentFill(s: Segment, t: number): number {
  if (t <= s.start) return 0;
  if (t >= s.end) return 1;
  return (t - s.start) / (s.end - s.start);
}
