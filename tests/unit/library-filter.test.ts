import { describe, expect, it } from "vitest";
import type { CourseSummary } from "@/bindings";
import { filterCourses } from "@/features/library/filter";
import { keyOf } from "@/lib/hotkeys";

function summary(
  id: string,
  over: Partial<CourseSummary> = {},
  status: "active" | "archived" | "completed" = "active",
): CourseSummary {
  return {
    course: {
      id,
      title: id,
      slug: id,
      description: null,
      instructor: null,
      sourceUrl: null,
      youtubePlaylistId: null,
      coverPath: null,
      tags: [],
      mode: "stream",
      status,
      playbackRate: 1,
      createdAt: "2026-01-01",
      updatedAt: "2026-01-01",
    },
    lessonCount: 10,
    completedCount: 0,
    totalDurationSec: 0,
    nextLessonId: null,
    nextLessonTitle: null,
    lastStudiedAt: null,
    ...over,
  };
}

describe("filterCourses", () => {
  const list = [
    summary("fresh"),
    summary("going", { completedCount: 3, lastStudiedAt: "2026-02-01" }),
    summary("done", { completedCount: 10 }),
    summary("old", {}, "archived"),
  ];
  const ids = (f: Parameters<typeof filterCourses>[1]) =>
    filterCourses(list, f, "title").map((c) => c.course.id);
  it("filters by status", () => {
    expect(ids("all")).toEqual(["done", "fresh", "going"]);
    expect(ids("in_progress")).toEqual(["going"]);
    expect(ids("not_started")).toEqual(["fresh"]);
    expect(ids("completed")).toEqual(["done"]);
    expect(ids("archived")).toEqual(["old"]);
  });
});

describe("keyOf", () => {
  it("normalises keys", () => {
    expect(keyOf({ key: " ", metaKey: false, ctrlKey: false, altKey: false })).toBe("space");
    expect(keyOf({ key: "K", metaKey: true, ctrlKey: false, altKey: false })).toBe("mod+k");
    expect(keyOf({ key: "ArrowLeft", metaKey: false, ctrlKey: false, altKey: false })).toBe("arrowleft");
  });
});
