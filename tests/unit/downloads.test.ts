import { describe, expect, it } from "vitest";
import type { Download } from "@/bindings";
import { formatBytes, formatEta, isActive, upsertDownload } from "@/features/downloads/useDownloads";
import { estimateBytes } from "@/features/downloads/quality";
import { ATTACHMENT_SCHEME, attachmentMarkdown, resolveImageSrc } from "@/lib/attachments";

const row = (id: string, status: Download["status"], progress = 0): Download => ({
  id,
  lessonId: `l-${id}`,
  lessonTitle: id,
  courseId: "c",
  courseTitle: "C",
  courseSlug: "c",
  localQuality: null,
  youtubeVideoId: "v",
  durationSec: 60,
  status,
  quality: "720p",
  progress,
  bytesDone: null,
  bytesTotal: null,
  speedBps: null,
  etaSec: null,
  errorCode: null,
  errorMessage: null,
  attempts: 0,
  nextRetryAt: null,
  createdAt: "",
  updatedAt: "",
});

describe("download store", () => {
  it("upserts pushed rows", () => {
    expect(upsertDownload(undefined, row("a", "queued"))).toHaveLength(1);
    const list = [row("a", "queued"), row("b", "running", 0.2)];
    const next = upsertDownload(list, row("b", "running", 0.5));
    expect(next[1]!.progress).toBe(0.5);
    expect(list[1]!.progress).toBe(0.2);
    expect(upsertDownload(list, row("c", "queued"))).toHaveLength(3);
    expect(list.filter(isActive)).toHaveLength(2);
  });
  it("formats sizes and ETA", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1_500_000)).toBe("1.5 MB");
    expect(formatBytes(156_552_437)).toBe("157 MB");
    expect(formatBytes(null)).toBe("");
    expect(formatEta(42)).toBe("42s left");
    expect(formatEta(600)).toBe("10 min left");
    expect(formatEta(3900)).toBe("1h 5m left");
  });
});

describe("attachments", () => {
  it("builds markdown and refuses traversal", () => {
    expect(attachmentMarkdown("media/c/l/frames/x.png", "Frame 1:05")).toBe(
      `![Frame 1:05](${ATTACHMENT_SCHEME}media/c/l/frames/x.png)`,
    );
    expect(resolveImageSrc("https://example.com/a.png")).toBe("https://example.com/a.png");
    expect(resolveImageSrc(`${ATTACHMENT_SCHEME}../../etc/passwd`)).toBe("");
  });
});

describe("estimateBytes", () => {
  it("uses the per-quality bitrate", () => {
    expect(estimateBytes(3600, "720p")).toBe((3600 * 1.8e6) / 8);
    expect(estimateBytes(null, "720p")).toBeNull();
    expect(estimateBytes(0, "360p")).toBeNull();
  });
});
