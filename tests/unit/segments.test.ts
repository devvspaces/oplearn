import { describe, expect, it } from "vitest";
import { buildSegments, chapterAt, segmentFill, timeAtPointer } from "@/features/player/segments";
import { plainText } from "@/features/annotations/timeline";

const ch = (startSec: number, title: string, endSec: number | null = null) => ({ startSec, endSec, title });

describe("buildSegments", () => {
  it("returns a single segment without chapters", () => {
    expect(buildSegments([], 100)).toEqual([{ start: 0, end: 100, title: null, leftPct: 0, widthPct: 100 }]);
    expect(buildSegments([], 0)).toEqual([]);
  });
  it("sorts, clips, de-duplicates and fills to the duration", () => {
    const s = buildSegments([ch(50, "B"), ch(0, "A"), ch(0, "dup"), ch(150, "past end")], 100);
    expect(s.map((x) => [x.start, x.end, x.title])).toEqual([
      [0, 50, "A"],
      [50, 100, "B"],
    ]);
    expect(s[1]!.leftPct).toBe(50);
    expect(s.reduce((a, x) => a + x.widthPct, 0)).toBeCloseTo(100);
  });
  it("adds a lead-in when the first chapter starts late", () => {
    const s = buildSegments([ch(20, "Intro")], 100);
    expect(s.map((x) => x.title)).toEqual([null, "Intro"]);
  });
  it("finds the chapter at a time and fill amounts", () => {
    const s = buildSegments([ch(0, "A"), ch(478, "Overview"), ch(732, "Labs")], 4775);
    expect(chapterAt(s, 500)?.title).toBe("Overview");
    expect(chapterAt(s, 0)?.title).toBe("A");
    expect(chapterAt(s, 9999)?.title).toBe("Labs");
    expect(segmentFill(s[1]!, 605)).toBeCloseTo(0.5);
    expect(segmentFill(s[1]!, 10)).toBe(0);
    expect(segmentFill(s[1]!, 800)).toBe(1);
  });
  it("maps pointer position to time", () => {
    expect(timeAtPointer(50, 200, 100)).toBe(25);
    expect(timeAtPointer(-10, 200, 100)).toBe(0);
    expect(timeAtPointer(500, 200, 100)).toBe(100);
    expect(timeAtPointer(10, 0, 100)).toBe(0);
  });
});

describe("plainText", () => {
  it("strips markdown for previews", () => {
    expect(plainText("# Title\n\n**Raft** uses `terms` and [logs](http://x)\n- item")).toBe(
      "Title Raft uses terms and logs item",
    );
    expect(plainText("> quoted text")).toBe("quoted text");
    expect(plainText(null)).toBe("");
    expect(plainText("a".repeat(200), 10)).toBe(`${"a".repeat(9)}…`);
  });
});
