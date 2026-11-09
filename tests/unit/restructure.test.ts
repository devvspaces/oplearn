import { describe, expect, it } from "vitest";
import { moveLessonTo } from "@/features/ai/RestructureDialog";

describe("moveLessonTo", () => {
  const mods = [
    { title: "A", summary: null, lessonIds: ["1", "2"] },
    { title: "B", summary: null, lessonIds: ["3"] },
  ];
  it("moves a lesson to the end of another module", () => {
    expect(moveLessonTo(mods, "1", 1).map((m) => m.lessonIds)).toEqual([["2"], ["3", "1"]]);
  });
  it("drops modules that become empty", () => {
    const r = moveLessonTo(mods, "3", 0);
    expect(r).toHaveLength(1);
    expect(r[0]!.lessonIds).toEqual(["1", "2", "3"]);
  });
});
