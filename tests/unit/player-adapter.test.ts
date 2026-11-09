import { describe, expect, it, vi } from "vitest";
import { clampRate, PositionSaver, resumePosition } from "@/lib/player-adapter";

describe("resumePosition", () => {
  it("rewinds 5 seconds for context", () => {
    expect(resumePosition(120, 4800)).toBe(115);
    expect(resumePosition(3, 4800)).toBe(0);
  });
  it("starts from the beginning when unwatched or finished", () => {
    expect(resumePosition(null, 4800)).toBe(0);
    expect(resumePosition(0, 4800)).toBe(0);
    expect(resumePosition(4797, 4800)).toBe(0);
  });
});

describe("clampRate", () => {
  it("snaps to 0.25 steps within 0.5–3", () => {
    expect(clampRate(1.1)).toBe(1);
    expect(clampRate(1.4)).toBe(1.5);
    expect(clampRate(0.25)).toBe(0.5);
    expect(clampRate(5)).toBe(3);
  });
});

describe("PositionSaver", () => {
  it("throttles saves while playing and flushes the latest position", () => {
    let now = 0;
    const save = vi.fn();
    const s = new PositionSaver(save, 10_000, () => now);
    now = 10_000;
    s.update(1); // first save after interval
    expect(save).toHaveBeenLastCalledWith(1);
    now = 15_000;
    s.update(6);
    expect(save).toHaveBeenCalledTimes(1);
    now = 20_000;
    s.update(11);
    expect(save).toHaveBeenCalledTimes(2);
    s.update(12);
    s.flush(); // pause / close
    expect(save).toHaveBeenLastCalledWith(12);
    s.flush(); // nothing new
    expect(save).toHaveBeenCalledTimes(3);
  });
  it("ignores invalid times", () => {
    const save = vi.fn();
    const s = new PositionSaver(save, 0, () => 1);
    s.update(Number.NaN);
    s.update(-1);
    s.flush();
    expect(save).not.toHaveBeenCalled();
  });
});
