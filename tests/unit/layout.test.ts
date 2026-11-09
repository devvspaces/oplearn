import { describe, expect, it } from "vitest";
import { findContainer, moveLesson, sameLayout, type Layout } from "@/features/course/layout";

const base: Layout = [
  { moduleId: "m1", lessonIds: ["a", "b", "c"] },
  { moduleId: "m2", lessonIds: ["d"] },
  { moduleId: "m3", lessonIds: [] },
];

describe("outline layout", () => {
  it("finds containers", () => {
    expect(findContainer(base, "b")).toBe("m1");
    expect(findContainer(base, "m3")).toBe("m3");
    expect(findContainer(base, "zz")).toBeUndefined();
  });
  it("reorders within a module (down and up)", () => {
    expect(moveLesson(base, "a", "c")[0]!.lessonIds).toEqual(["b", "c", "a"]);
    expect(moveLesson(base, "c", "a")[0]!.lessonIds).toEqual(["c", "a", "b"]);
    expect(moveLesson(base, "a", "b")[0]!.lessonIds).toEqual(["b", "a", "c"]);
  });
  it("moves across modules, including into empty ones", () => {
    const l = moveLesson(base, "b", "d");
    expect(l[0]!.lessonIds).toEqual(["a", "c"]);
    expect(l[1]!.lessonIds).toEqual(["b", "d"]);
    const e = moveLesson(base, "a", "m3");
    expect(e[2]!.lessonIds).toEqual(["a"]);
  });
  it("is a no-op for self or unknown targets and doesn't mutate input", () => {
    expect(moveLesson(base, "a", "a")).toBe(base);
    expect(moveLesson(base, "a", "nope")).toBe(base);
    expect(base[0]!.lessonIds).toEqual(["a", "b", "c"]);
    expect(sameLayout(base, structuredClone(base))).toBe(true);
    expect(sameLayout(base, moveLesson(base, "a", "c"))).toBe(false);
  });
});
