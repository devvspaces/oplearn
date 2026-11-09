import { useQueryClient } from "@tanstack/react-query";
import { ask } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import type { Quality } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { downloadsKey } from "./useDownloads";

export const DOWNLOAD_NOTICE =
  "YouTube's Terms generally allow downloading only where the content's license or owner permits it. " +
  "You are responsible for downloading only content you have the right to, for example Creative Commons " +
  "lectures, your own videos, or videos whose creator allows it. Downloads are for your personal study and " +
  "must not be redistributed.";

/** Shows the one-time ToS notice (SPEC §4.2) if it hasn't been acknowledged. Returns false if declined. */
export async function ensureDownloadNotice(): Promise<boolean> {
  const s = await commands.getDownloadSettings();
  if (s.noticeAcknowledged) return true;
  const ok = await ask(DOWNLOAD_NOTICE, {
    title: "Before you download",
    kind: "warning",
    okLabel: "I understand",
    cancelLabel: "Cancel",
  });
  if (ok) await commands.setDownloadSettings({ ...s, noticeAcknowledged: true });
  return ok;
}

/** Queues one lesson at a chosen quality (null = default), after the ToS notice. */
export function useLessonDownload() {
  const qc = useQueryClient();
  return async (lessonId: string, quality: Quality | null) => {
    try {
      if (!(await ensureDownloadNotice())) return;
      await commands.enqueueLessonDownload(lessonId, quality);
      toast.success(quality ? `Queued for download at ${quality}` : "Queued for download");
      void qc.invalidateQueries({ queryKey: downloadsKey });
      void qc.invalidateQueries({ queryKey: ["downloadSettings"] });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };
}
