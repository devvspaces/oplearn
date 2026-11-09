import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Sparkles } from "lucide-react";
import type { FlashcardDraft } from "@/bindings";
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
import { Textarea } from "@/components/ui/input";
import { Spinner } from "@/components/ui/misc";
import { CostLine, money, overBudget } from "@/features/ai/CostLine";
import { useAiStatus } from "@/features/ai/AiSettings";
import { fk } from "./api";
import { anchorLabel, cardAnchor } from "@/lib/anchors";

/** AI flashcards for one lesson: estimate → drafts (editable) → save the chosen ones. */
export function GenerateCardsDialog({
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
  const [count, setCount] = useState(10);
  const [notes, setNotes] = useState(true);
  const [drafts, setDrafts] = useState<FlashcardDraft[] | null>(null);
  const [keep, setKeep] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const estimate = useQuery({
    queryKey: ["cardsEstimate", lessonId, count, notes, ai?.settings.model],
    queryFn: () => commands.estimateFlashcards(lessonId, count, notes),
    enabled: open && ready && !drafts,
    retry: false,
  });
  const close = (o: boolean) => {
    if (busy) return;
    onOpenChange(o);
    if (!o) setDrafts(null);
  };

  async function generate() {
    setBusy(true);
    try {
      const d = await commands.generateFlashcards(lessonId, count, notes);
      setDrafts(d);
      setKeep(new Set(d.map((_, i) => i)));
      void qc.invalidateQueries({ queryKey: ["aiUsage"] });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (!drafts) return;
    setBusy(true);
    try {
      const n = await commands.saveFlashcardDrafts(
        lessonId,
        drafts.filter((_, i) => keep.has(i)),
      );
      toast.success(`Added ${n} flashcard${n === 1 ? "" : "s"}`);
      void qc.invalidateQueries({ queryKey: fk.all });
      void qc.invalidateQueries({ queryKey: ["study"] });
      onOpenChange(false);
      setDrafts(null);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  const edit = (i: number, patch: Partial<FlashcardDraft>) =>
    setDrafts((d) => d?.map((x, j) => (j === i ? { ...x, ...patch } : x)) ?? null);

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="max-h-[90vh] max-w-2xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-5 text-primary" /> Generate flashcards
          </DialogTitle>
          <DialogDescription>
            From this lesson's transcript{notes ? ", your notes" : ""} and any lecture notes. Each card links
            to the moment it comes from. Edit or untick before saving.
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 overflow-y-auto">
          {!ready ? (
            <p className="rounded-lg border bg-muted/50 p-4 text-sm">
              Turn on AI and add your Anthropic API key in{" "}
              <Link
                to="/settings"
                className="text-primary hover:underline"
                onClick={() => onOpenChange(false)}
              >
                Settings → AI
              </Link>
              .
            </p>
          ) : !drafts ? (
            <div className="space-y-4 text-sm">
              <div className="flex flex-wrap items-center gap-6">
                <label className="flex items-center gap-2">
                  Cards
                  <select
                    value={count}
                    onChange={(e) => setCount(Number(e.target.value))}
                    className="rounded-md border bg-background px-2 py-1 text-xs"
                  >
                    {[5, 10, 15, 20, 30].map((n) => (
                      <option key={n}>{n}</option>
                    ))}
                  </select>
                </label>
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={notes} onChange={(e) => setNotes(e.target.checked)} />{" "}
                  Include my notes
                </label>
              </div>
              <div className="rounded-lg border p-3">
                {estimate.isError ? (
                  <p className="text-xs text-destructive">{errorMessage(estimate.error)}</p>
                ) : (
                  <CostLine
                    estimate={estimate.data}
                    what={estimate.data ? `${estimate.data.items} cards` : undefined}
                  />
                )}
              </div>
            </div>
          ) : (
            <ul className="space-y-2">
              {drafts.map((d, i) => (
                <li key={i} className="flex gap-2 rounded-lg border p-2">
                  <input
                    type="checkbox"
                    className="mt-2"
                    aria-label={`Keep card ${i + 1}`}
                    checked={keep.has(i)}
                    onChange={(e) =>
                      setKeep((k) => {
                        const n = new Set(k);
                        if (e.target.checked) n.add(i);
                        else n.delete(i);
                        return n;
                      })
                    }
                  />
                  <div className="grid min-w-0 flex-1 gap-1">
                    <Textarea
                      rows={2}
                      value={d.frontMd}
                      onChange={(e) => edit(i, { frontMd: e.target.value })}
                      aria-label="Front"
                      className="text-sm font-medium"
                    />
                    <Textarea
                      rows={2}
                      value={d.backMd}
                      onChange={(e) => edit(i, { backMd: e.target.value })}
                      aria-label="Back"
                      className="text-xs"
                    />
                    {anchorLabel(cardAnchor(d)) && (
                      <span className="font-mono text-[10px] text-primary">
                        from {anchorLabel(cardAnchor(d))}
                      </span>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => close(false)} disabled={busy}>
            Cancel
          </Button>
          {drafts ? (
            <Button onClick={() => void save()} disabled={busy || keep.size === 0}>
              Save {keep.size} card{keep.size === 1 ? "" : "s"}
            </Button>
          ) : (
            <Button
              onClick={() => void generate()}
              disabled={!ready || busy || !estimate.data || overBudget(estimate.data)}
            >
              {busy ? (
                <>
                  <Spinner /> Writing cards…
                </>
              ) : (
                <>
                  <Sparkles /> Generate {estimate.data ? `(~${money(estimate.data.costUsd)})` : ""}
                </>
              )}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
