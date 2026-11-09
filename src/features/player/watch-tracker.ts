/**
 * Turns a stream of player time updates into watched segments (SPEC §9).
 *
 * Playback is "continuous" while each update advances by less than 2 s × rate; anything else
 * (seek, skip, rewind) closes the segment and starts a new one. Wall-clock seconds are counted
 * separately because study minutes are real time, not video time.
 */
export interface WatchBatch {
  segments: Array<{ from: number; to: number }>;
  wallSecs: number;
}

export class WatchTracker {
  private segs: Array<{ from: number; to: number }> = [];
  private cur: { from: number; last: number } | null = null;
  private lastWall: number | null = null;
  private wall = 0;

  constructor(private rate: () => number = () => 1) {}

  /** `counting` = playing and the window is focused (or in picture-in-picture). */
  tick(t: number, nowMs: number, counting: boolean) {
    if (!Number.isFinite(t)) return;
    if (!counting) {
      this.close();
      this.lastWall = null;
      return;
    }
    const maxStep = 2 * Math.max(0.25, this.rate());
    if (this.cur && t >= this.cur.last && t - this.cur.last < maxStep) {
      this.cur.last = t;
      if (this.lastWall != null) this.wall += Math.min(nowMs - this.lastWall, 2000) / 1000;
    } else {
      this.close();
      this.cur = { from: t, last: t };
    }
    this.lastWall = nowMs;
  }

  /** Ends the current segment (pause, seek, lesson change). */
  close() {
    if (this.cur && this.cur.last > this.cur.from) this.segs.push({ from: this.cur.from, to: this.cur.last });
    this.cur = null;
  }

  /** Returns and clears everything recorded so far; an open segment continues from where it is. */
  take(): WatchBatch | null {
    const open = this.cur;
    if (open && open.last > open.from) {
      this.segs.push({ from: open.from, to: open.last });
      this.cur = { from: open.last, last: open.last };
    }
    const wallSecs = Math.round(this.wall * 10) / 10;
    if (!this.segs.length && wallSecs < 0.5) return null;
    const out = { segments: this.segs, wallSecs };
    this.segs = [];
    this.wall = 0;
    return out;
  }
}
