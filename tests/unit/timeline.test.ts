import { describe, expect, it } from "vitest";
import { currentCueIndex, findCues } from "@/features/transcript/cues";
import { groupMarkers, isNear, markerPercent } from "@/features/annotations/timeline";

const cues = [
  { start: 1.68, text: "All right." },
  { start: 3.32, text: "Let's get started." },
  { start: 6.04, text: "This is 6824 distributed systems." },
  { start: 9.48, text: "Um" },
];

describe("currentCueIndex", () => {
  it("finds the cue being spoken", () => {
    expect(currentCueIndex(cues, 0)).toBe(-1);
    expect(currentCueIndex(cues, 1.68)).toBe(0);
    expect(currentCueIndex(cues, 5)).toBe(1);
    expect(currentCueIndex(cues, 6.04)).toBe(2);
    expect(currentCueIndex(cues, 9999)).toBe(3);
    expect(currentCueIndex([], 5)).toBe(-1);
  });
  it("searches cue text", () => {
    expect(findCues(cues, "DISTRIBUTED")).toEqual([2]);
    expect(findCues(cues, "t")).toEqual([0, 1, 2]);
    expect(findCues(cues, "  ")).toEqual([]);
  });
});

describe("timeline markers", () => {
  it("computes positions", () => {
    expect(markerPercent(30, 120)).toBe(25);
    expect(markerPercent(500, 120)).toBe(100);
    expect(markerPercent(null, 120)).toBeNull();
    expect(markerPercent(30, 0)).toBeNull();
  });
  it("highlights items near the playhead", () => {
    expect(isNear({ startSec: 100, endSec: null }, 110)).toBe(true);
    expect(isNear({ startSec: 100, endSec: null }, 120)).toBe(false);
    expect(isNear({ startSec: 100, endSec: 200 }, 210)).toBe(true);
    expect(isNear({ startSec: null, endSec: null }, 0)).toBe(false);
  });
  it("groups overlapping markers", () => {
    const items = [{ startSec: 10 }, { startSec: 10.5 }, { startSec: 600 }, { startSec: null }];
    const g = groupMarkers(items, 4800);
    expect(g).toHaveLength(2);
    expect(g[0]!.items).toHaveLength(2);
    expect(g[1]!.percent).toBeCloseTo(12.5);
  });
});
