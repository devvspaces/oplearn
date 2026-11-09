import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { events, type Download } from "@/bindings";
import { commands } from "@/lib/api";

export const downloadsKey = ["downloads"] as const;

export const useDownloadsList = () => useQuery({ queryKey: downloadsKey, queryFn: commands.listDownloads });

/** Merges a pushed row into the cached list (pure; unit-tested). */
export function upsertDownload(list: Download[] | undefined, row: Download): Download[] {
  if (!list) return [row];
  const i = list.findIndex((d) => d.id === row.id);
  if (i < 0) return [...list, row];
  const next = list.slice();
  next[i] = row;
  return next;
}

export const isActive = (d: Download) => d.status === "running" || d.status === "queued";

export function useActiveDownloads(): number {
  const { data } = useDownloadsList();
  return (data ?? []).filter(isActive).length;
}

/** Subscribes once (in the root layout) to backend download events. */
export function useDownloadEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const unsubs: Array<Promise<() => void>> = [
      events.downloadUpdate.listen((e) => {
        qc.setQueryData<Download[]>(downloadsKey, (list) => upsertDownload(list, e.payload));
      }),
      events.downloadsChanged.listen(() => void qc.invalidateQueries({ queryKey: downloadsKey })),
      events.lessonMediaChanged.listen(() => {
        void qc.invalidateQueries({ queryKey: ["course"] });
        void qc.invalidateQueries({ queryKey: ["lesson"] });
        void qc.invalidateQueries({ queryKey: ["courses"] });
        void qc.invalidateQueries({ queryKey: ["storage"] });
      }),
    ];
    return () => unsubs.forEach((u) => void u.then((f) => f()));
  }, [qc]);
}

export function formatBytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n) || n < 0) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function formatEta(sec: number | null | undefined): string {
  if (sec == null || !Number.isFinite(sec)) return "";
  if (sec < 60) return `${Math.round(sec)}s left`;
  const m = Math.round(sec / 60);
  return m < 60 ? `${m} min left` : `${Math.floor(m / 60)}h ${m % 60}m left`;
}
