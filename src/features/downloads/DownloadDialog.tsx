import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ShieldAlert } from "lucide-react";
import type { Quality } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { downloadsKey, formatBytes } from "./useDownloads";

const QUALITIES: Quality[] = ["360p", "480p", "720p", "1080p"];

/** Confirms switching a course to Download mode: quality, size estimate, and the one-time ToS notice. */
export function DownloadDialog({
  courseId,
  open,
  onOpenChange,
}: {
  courseId: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const qc = useQueryClient();
  const settings = useQuery({
    queryKey: ["downloadSettings"],
    queryFn: commands.getDownloadSettings,
    enabled: open,
  });
  const [quality, setQuality] = useState<Quality>("720p");
  const [ack, setAck] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (settings.data) {
      setQuality(settings.data.quality);
      setAck(settings.data.noticeAcknowledged);
    }
  }, [settings.data]);
  const estimate = useQuery({
    queryKey: ["estimate", courseId, quality],
    queryFn: () => commands.estimateDownloadSize(courseId, quality),
    enabled: open,
  });
  const needsNotice = settings.data && !settings.data.noticeAcknowledged;

  async function confirm() {
    if (!settings.data) return;
    setBusy(true);
    try {
      if (needsNotice) await commands.setDownloadSettings({ ...settings.data, noticeAcknowledged: true });
      const n = await commands.setCourseMode(courseId, "download", quality);
      toast.success(n ? `Queued ${n} lessons for download` : "Everything is already downloaded");
      void qc.invalidateQueries({ queryKey: ["course"] });
      void qc.invalidateQueries({ queryKey: ["downloadSettings"] });
      void qc.invalidateQueries({ queryKey: downloadsKey });
      onOpenChange(false);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Download this course for offline study</DialogTitle>
          <DialogDescription>
            Lessons are saved to your Lectern data folder with their subtitles. Notes and progress work the
            same offline.
          </DialogDescription>
        </DialogHeader>

        {needsNotice && (
          <div className="rounded-lg border border-warning/40 bg-warning/10 p-3 text-sm">
            <p className="flex items-center gap-2 font-medium">
              <ShieldAlert className="size-4 text-warning" /> Before you download
            </p>
            <p className="mt-1.5 text-muted-foreground">
              YouTube's Terms generally allow downloading only where the content's license or owner permits
              it. You are responsible for downloading only content you have the right to, for example Creative
              Commons lectures (such as MIT OpenCourseWare), your own videos, or videos whose creator allows
              it. Downloads are for your personal study and must not be redistributed.
            </p>
            <label className="mt-2 flex items-center gap-2">
              <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />I understand
            </label>
          </div>
        )}

        <div className="grid gap-2">
          <span className="text-sm font-medium">Quality</span>
          <div role="radiogroup" aria-label="Quality" className="flex w-fit gap-1 rounded-lg bg-muted p-1">
            {QUALITIES.map((q) => (
              <button
                key={q}
                role="radio"
                aria-checked={quality === q}
                onClick={() => setQuality(q)}
                className={
                  "rounded-md px-3 py-1 text-xs font-medium " +
                  (quality === q ? "bg-background text-foreground shadow-sm" : "text-muted-foreground")
                }
              >
                {q}
              </button>
            ))}
          </div>
          <p className="text-xs text-muted-foreground">
            {estimate.data
              ? estimate.data.lessons === 0
                ? "All lessons are already downloaded."
                : `${estimate.data.lessons} lessons · about ${formatBytes(estimate.data.bytes)} (estimate)` +
                  (estimate.data.unknown ? ` + ${estimate.data.unknown} of unknown length` : "")
              : "Estimating size…"}
          </p>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={confirm} disabled={busy || !settings.data || (needsNotice && !ack)}>
            {busy ? "Queuing…" : "Download"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
