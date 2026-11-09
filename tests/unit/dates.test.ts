import { describe, expect, it } from "vitest";
import {
  addDays,
  addMonths,
  heatLevel,
  monthGrid,
  relativeDay,
  sameMonth,
  toWeeks,
  weekStart,
} from "@/lib/dates";

describe("dates", () => {
  it("adds days across months and finds Monday", () => {
    expect(addDays("2026-11-30", 1)).toBe("2026-12-01");
    expect(weekStart("2026-11-08")).toBe("2026-11-02"); // Sunday → previous Monday
    expect(weekStart("2026-11-09")).toBe("2026-11-09");
  });
  it("relative days", () => {
    expect(relativeDay("2026-11-08", "2026-11-08")).toBe("Today");
    expect(relativeDay("2026-11-09", "2026-11-08")).toBe("Tomorrow");
  });
  it("heat levels relative to goal", () => {
    expect([0, 5, 20, 40, 45, 90].map((m) => heatLevel(m, 45))).toEqual([0, 1, 2, 3, 4, 4]);
  });
  it("buckets into Monday-first weeks", () => {
    const days = ["2026-11-04", "2026-11-05", "2026-11-06", "2026-11-07", "2026-11-08", "2026-11-09"].map(
      (date) => ({ date }),
    );
    const w = toWeeks(days);
    expect(w).toHaveLength(2);
    expect(w[0]!.slice(0, 2)).toEqual([null, null]); // Wed start → Mon/Tue padded
    expect(w[1]![0]!.date).toBe("2026-11-09");
  });
});

describe("month grid", () => {
  it("covers the month Monday-first in 6 weeks", () => {
    const g = monthGrid("2026-11-18");
    expect(g).toHaveLength(6);
    expect(g[0]![0]).toBe("2026-10-26"); // Nov 1 2026 is a Sunday
    expect(g[0]![6]).toBe("2026-11-01");
    expect(g.flat()).toContain("2026-11-30");
    expect(addMonths("2026-11-18", 1)).toBe("2026-12-01");
    expect(addMonths("2026-01-31", -1)).toBe("2025-12-01");
    expect(sameMonth("2026-11-01", "2026-11-30")).toBe(true);
  });
});
