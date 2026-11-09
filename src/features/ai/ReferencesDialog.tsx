import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { BookMarked, Search, Sparkles } from "lucide-react";
import type { ReferenceSuggestions } from "@/bindings";
import { commands, errorMessage, qk } from "@/lib/api";
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
import { CostLine, money, overBudget } from "./CostLine";
import { useAiStatus } from "./AiSettings";

export const webSearch = (q: string) =>
  void openUrl(`https://duckduckgo.com/?q=${encodeURIComponent(q)}`);

/** AI reading suggestions for a lesson: titles and authors only (no invented links). */
export function ReferencesDialog({
  lessonId,
  open,
  onOpenChange,
}: {
  lessonId: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const qc = useQueryClient();
  const { data: ai } = useAiStatus();
  const ready = !!ai?.settings.enabled && ai.hasKey;
  const [res, setRes] = useState<ReferenceSuggestions | null>(null);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) setRes(null);
  }, [open]);
  const estimate = useQuery({
    queryKey: ["refsEstimate", lessonId, ai?.settings.model],
    queryFn: () => commands.estimateReferences(lessonId),
    enabled: open && ready && !res,
    retry: false,
  });

  async function generate() {
    setBusy(true);
    try {
      const r = await commands.suggestReferences(lessonId);
      setRes(r);
      setPicked(new Set(r.items.map((_, i) => i)));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
      void qc.invalidateQueries({ queryKey: ["aiUsage"] });
    }
  }
  async function save() {
    if (!res) return;
    setBusy(true);
    try {
      const n = await commands.saveReferenceDrafts(
        lessonId,
        res.items.filter((_, i) => picked.has(i)),
      );
      void qc.invalidateQueries({ queryKey: qk.annotations(lessonId) });
      void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
      toast.success(`Added ${n} reference${n === 1 ? "" : "s"}`);
      onOpenChange(false);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent className="max-h-[85vh] max-w-xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <BookMarked className="size-5 text-primary" /> Suggest references
          </DialogTitle>
          <DialogDescription>
            Claude suggests textbooks, papers and articles for this lesson. Suggestions have no links (so none
            are made up): use Search to find each one.
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto text-sm">
          {!ready ? (
            <p className="rounded-lg border bg-muted/50 p-4">Turn on AI and add your API key in Settings → AI.</p>
          ) : res ? (
            <ul className="space-y-2">
              {res.items.map((r, i) => (
                <li key={i} className="flex gap-2 rounded-lg border p-2">
                  <input
                    type="checkbox"
                    className="mt-1"
                    aria-label={`Keep ${r.title}`}
                    checked={picked.has(i)}
                    onChange={(e) => {
                      const s = new Set(picked);
                      if (e.target.checked) s.add(i);
                      else s.delete(i);
                      setPicked(s);
                    }}
                  />
                  <div className="min-w-0 flex-1">
                    <p className="font-medium">
                      {r.title} <Badge tone="muted">{r.refType}</Badge>
                    </p>
                    {r.author && <p className="text-xs text-muted-foreground">{r.author}</p>}
                    <p className="mt-1 text-xs">{r.why}</p>
                  </div>
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    aria-label={`Search the web for ${r.title}`}
                    onClick={() => webSearch([r.title, r.author].filter(Boolean).join(" "))}
                  >
                    <Search />
                  </Button>
                </li>
              ))}
              <p className="text-xs text-muted-foreground">Cost {money(res.costUsd)}</p>
            </ul>
          ) : (
            <div className="rounded-lg border p-3">
              {estimate.isError ? (
                <p className="text-xs text-destructive">{errorMessage(estimate.error)}</p>
              ) : (
                <CostLine estimate={estimate.data} />
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          {res ? (
            <Button onClick={() => void save()} disabled={busy || picked.size === 0}>
              Add {picked.size} reference{picked.size === 1 ? "" : "s"}
            </Button>
          ) : (
            <Button
              onClick={() => void generate()}
              disabled={!ready || busy || !estimate.data || overBudget(estimate.data)}
            >
              {busy ? <Spinner /> : <Sparkles />} Suggest{" "}
              {estimate.data ? `(~${money(estimate.data.costUsd)})` : ""}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
