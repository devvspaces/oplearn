import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { commands, errorMessage, qk } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input, Label } from "@/components/ui/input";
import { Spinner } from "@/components/ui/misc";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export function ImportPlaylistDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const qc = useQueryClient();
  const navigate = useNavigate();

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const r = await commands.importPlaylist(url);
      await qc.invalidateQueries({ queryKey: qk.courses });
      toast.success(
        `Imported ${r.added} lessons${r.skipped ? ` (${r.skipped} unavailable skipped)` : ""}` +
          (r.source === "ytdlp" ? " via yt-dlp" : ""),
      );
      onOpenChange(false);
      setUrl("");
      void navigate({ to: "/courses/$courseId", params: { courseId: r.courseId } });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>Create course from playlist</DialogTitle>
            <DialogDescription>
              Paste a YouTube playlist URL. Each video becomes a lesson, in playlist order. You can split it
              into modules afterwards.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor="playlist-url">Playlist URL</Label>
            <Input
              id="playlist-url"
              autoFocus
              placeholder="https://www.youtube.com/playlist?list=…"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              disabled={busy}
            />
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            {busy && (
              <p className="flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner /> Fetching playlist… large playlists can take up to a minute without an API key.
              </p>
            )}
          </div>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !url.trim()}>
              {busy ? "Importing…" : "Import"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
