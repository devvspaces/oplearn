import { describe, expect, it } from "vitest";
import { formatDuration, formatTimestamp, relativeTime } from "@/lib/time";

describe("time", () => {
  it("formats timestamps", () => {
    expect(formatTimestamp(0)).toBe("0:00");
    expect(formatTimestamp(75)).toBe("1:15");
    expect(formatTimestamp(754.9)).toBe("12:34");
    expect(formatTimestamp(3723)).toBe("1:02:03");
    expect(formatTimestamp(-5)).toBe("0:00");
  });
  it("formats durations", () => {
    expect(formatDuration(null)).toBe("");
    expect(formatDuration(42)).toBe("42s");
    expect(formatDuration(300)).toBe("5m");
    expect(formatDuration(4813)).toBe("1h 20m");
    expect(formatDuration(7200)).toBe("2h");
  });
  it("formats relative times", () => {
    const now = Date.parse("2026-01-10T12:00:00Z");
    expect(relativeTime("2026-01-10T11:59:30Z", now)).toBe("just now");
    expect(relativeTime("2026-01-10T10:00:00Z", now)).toBe("2h ago");
    expect(relativeTime("2026-01-09T10:00:00Z", now)).toBe("yesterday");
  });
});
