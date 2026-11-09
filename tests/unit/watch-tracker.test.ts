import { describe, expect, it } from "vitest";
import { WatchTracker } from "@/features/player/watch-tracker";

function play(w: WatchTracker, from: number, to: number, startMs: number, stepSec = 0.25) {
  let ms = startMs;
  for (let t = from; t <= to + 1e-9; t += stepSec) {
    w.tick(t, ms, true);
    ms += stepSec * 1000;
  }
  return ms;
}

describe("WatchTracker", () => {
  it("records continuous playback as one segment with wall time", () => {
    const w = new WatchTracker();
    play(w, 0, 10, 0);
    const b = w.take()!;
    expect(b.segments).toEqual([{ from: 0, to: 10 }]);
    expect(b.wallSecs).toBeCloseTo(10, 0);
    expect(w.take()).toBeNull();
  });

  it("splits on seeks and skips, and doesn't count the jump", () => {
    const w = new WatchTracker();
    const ms = play(w, 0, 5, 0);
    play(w, 590, 600, ms); // jumped to the end
    const b = w.take()!;
    expect(b.segments).toEqual([
      { from: 0, to: 5 },
      { from: 590, to: 600 },
    ]);
    expect(b.wallSecs).toBeCloseTo(15, 0);
  });

  it("allows bigger steps at higher speed", () => {
    const w = new WatchTracker(() => 2);
    let ms = 0;
    for (let t = 0; t <= 20; t += 3) {
      w.tick(t, ms, true);
      ms += 1500;
    }
    expect(w.take()!.segments).toEqual([{ from: 0, to: 18 }]);
  });

  it("stops counting when not focused or paused, and keeps an open segment across takes", () => {
    const w = new WatchTracker();
    let ms = play(w, 0, 4, 0);
    expect(w.take()!.segments).toEqual([{ from: 0, to: 4 }]);
    ms = play(w, 4.25, 8, ms);
    w.tick(9, ms + 250, false); // window lost focus
    w.tick(30, ms + 20000, false);
    const b = w.take()!;
    expect(b.segments).toEqual([{ from: 4, to: 8 }]);
    expect(b.wallSecs).toBeLessThan(5);
  });
});
