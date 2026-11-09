import type { TranscriptCue } from "@/bindings";

/**
 * Index of the cue being spoken at `t`: the last cue whose start ≤ t (binary search).
 * Returns -1 before the first cue. Cues are sorted by start.
 */
export function currentCueIndex(cues: readonly Pick<TranscriptCue, "start">[], t: number): number {
  let lo = 0;
  let hi = cues.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if ((cues[mid]!.start ?? 0) <= t) {
      ans = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return ans;
}

/** Indices of cues containing `query` (case-insensitive). */
export function findCues(cues: readonly Pick<TranscriptCue, "text">[], query: string): number[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const out: number[] = [];
  cues.forEach((c, i) => {
    if (c.text.toLowerCase().includes(q)) out.push(i);
  });
  return out;
}
