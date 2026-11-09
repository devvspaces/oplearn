/**
 * One interface for every playback backend (SPEC §7.4): the YouTube IFrame player in
 * Stream mode, and the local <video> element in Download mode (Phase 3).
 */
export interface PlayerAdapter {
  play(): void;
  pause(): void;
  seek(seconds: number): void;
  getTime(): number;
  getDuration(): number;
  isPlaying(): boolean;
  setRate(rate: number): void;
  getRate(): number;
  setVolume(volume: number): void;
  /** Called with the current time roughly every 250 ms while playing, and after seeks. */
  onTimeUpdate(cb: (seconds: number) => void): () => void;
  onStateChange(cb: (state: PlayerState) => void): () => void;
  destroy(): void;
}

export type PlayerState = "unstarted" | "playing" | "paused" | "buffering" | "ended";

export const MIN_RATE = 0.5;
export const MAX_RATE = 3;
export const RATE_STEP = 0.25;

export function clampRate(rate: number): number {
  const snapped = Math.round(rate / RATE_STEP) * RATE_STEP;
  return Math.min(MAX_RATE, Math.max(MIN_RATE, snapped));
}

/** Resume point: rewind 5 s for context; restart if the lesson was essentially finished. */
export function resumePosition(lastSec: number | null | undefined, durationSec?: number | null): number {
  if (!lastSec || lastSec < 0) return 0;
  if (durationSec && lastSec >= durationSec - 5) return 0;
  return Math.max(0, lastSec - 5);
}

/**
 * Decides when to persist the playback position: at most every `intervalMs` while playing,
 * plus explicit flushes (pause, lesson switch, app close). Pure apart from the injected clock.
 */
export class PositionSaver {
  private last: number | null = null;
  private lastSaved: number | null = null;
  private lastSaveAt = 0;

  constructor(
    private readonly save: (seconds: number) => Promise<unknown> | void,
    private readonly intervalMs = 10_000,
    private readonly now: () => number = () => Date.now(),
  ) {}

  update(seconds: number): void {
    if (!Number.isFinite(seconds) || seconds < 0) return;
    this.last = seconds;
    if (this.now() - this.lastSaveAt >= this.intervalMs) this.flush();
  }

  flush(): Promise<unknown> | void {
    if (this.last == null || this.last === this.lastSaved) return;
    this.lastSaved = this.last;
    this.lastSaveAt = this.now();
    return this.save(this.last);
  }
}
