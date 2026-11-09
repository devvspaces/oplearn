import type { Quality } from "@/bindings";

export const QUALITIES: Quality[] = ["360p", "480p", "720p", "1080p"];

/** Same heuristics as `Quality::est_bitrate` in Rust (bits per second, video + audio). */
const BITRATE: Record<Quality, number> = { "360p": 0.6e6, "480p": 1.0e6, "720p": 1.8e6, "1080p": 3.5e6 };

/** Rough download size in bytes, or null when the duration is unknown. */
export function estimateBytes(durationSec: number | null | undefined, q: Quality): number | null {
  if (!durationSec || durationSec <= 0) return null;
  return (durationSec * BITRATE[q]) / 8;
}
