import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
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
import { Badge, Spinner } from "@/components/ui/misc";
import { formatDuration } from "@/lib/time";

/** A playlist's videos, so the user can judge it before creating a course. */
export function PlaylistPreviewDialog({
  playlistId,
  onClose,
  onCreate,
}: {
  playlistId: string | null;
  onClose: () => void;
  onCreate: (id: string) => void;
}) {
  const q = useQuery({
    queryKey: ["playlistPreview", playlistId],
    queryFn: () => commands.previewPlaylist(playlistId!),
    enabled: !!playlistId,
    staleTime: 10 * 60_000,
    retry: false,
  });
  const p = q.data;
  const cc = p?.videos.filter((v) => v.license === "creativeCommon").length ?? 0;
  return (
    <Dialog open={!!playlistId} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[85vh] max-w-xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="line-clamp-2">{p?.title ?? "Playlist"}</DialogTitle>
          <DialogDescription>
            {p
              ? [
                  p.channelTitle,
                  `${p.videos.length} videos`,
                  p.totalSec ? formatDuration(p.totalSec) : null,
                  p.skipped ? `${p.skipped} unavailable` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")
              : "Loading the video list…"}
            {cc > 0 && (
              <Badge tone="success" className="ml-2">
                {cc === p?.videos.length ? "All CC" : `${cc} CC`}
              </Badge>
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 overflow-y-auto">
          {q.isLoading ? (
            <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
              <Spinner /> Fetching the playlist (this can take a few seconds)…
            </div>
          ) : q.isError ? (
            <p className="p-4 text-sm text-destructive">{errorMessage(q.error)}</p>
          ) : (
            <>
              {p?.description && (
                <p className="mb-3 line-clamp-4 whitespace-pre-wrap text-xs text-muted-foreground">
                  {p.description}
                </p>
              )}
              <ol className="divide-y rounded-lg border">
                {p?.videos.map((v, i) => (
                  <li key={v.id} className="flex items-center gap-3 px-3 py-2 text-sm">
                    <span className="w-6 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
                      {i + 1}
                    </span>
                    <span className="min-w-0 flex-1 truncate" title={v.title}>
                      {v.title}
                    </span>
                    {v.license === "creativeCommon" && <Badge tone="success">CC</Badge>}
                    <span className="w-14 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
                      {formatDuration(v.durationSec)}
                    </span>
                  </li>
                ))}
              </ol>
            </>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
          <Button disabled={!p} onClick={() => playlistId && onCreate(playlistId)}>
            <Plus /> Create course
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
