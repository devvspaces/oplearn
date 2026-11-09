import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label, Textarea } from "@/components/ui/input";
import { formatTimestamp } from "@/lib/time";
import { useCardDialog, useSaveCard } from "./api";

/** Mounted once in the root layout; opened via `useCardDialog.getState().open(...)`. */
export function CardDialog() {
  const { draft, close } = useCardDialog();
  const save = useSaveCard();
  const [front, setFront] = useState("");
  const [back, setBack] = useState("");
  useEffect(() => {
    setFront(draft?.front ?? "");
    setBack(draft?.back ?? "");
  }, [draft]);

  const submit = () => {
    if (!draft || !front.trim()) return;
    save.mutate(
      {
        id: draft.id,
        card: {
          lessonId: draft.lessonId ?? null,
          sourceSec: draft.sourceSec ?? null,
          sourceMaterialId: draft.sourceMaterialId ?? null,
          sourcePage: draft.sourcePage ?? null,
          frontMd: front,
          backMd: back,
        },
      },
      { onSuccess: close },
    );
  };

  return (
    <Dialog open={!!draft} onOpenChange={(o) => !o && close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{draft?.id ? "Edit flashcard" : "New flashcard"}</DialogTitle>
          <DialogDescription>
            {draft?.sourceSec != null
              ? `Linked to ${formatTimestamp(draft.sourceSec)} in the lesson, so you can jump back while reviewing.`
              : draft?.sourcePage != null
                ? `Linked to page ${draft.sourcePage} of the document, so you can jump back while reviewing.`
                : "Write a question on the front and the answer on the back. Markdown works."}
          </DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              submit();
            }
          }}
        >
          <div className="grid gap-1">
            <Label htmlFor="card-front">Front</Label>
            <Textarea
              id="card-front"
              autoFocus
              rows={3}
              value={front}
              onChange={(e) => setFront(e.target.value)}
              placeholder="What does Raft use to elect a leader?"
            />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="card-back">Back</Label>
            <Textarea
              id="card-back"
              rows={4}
              value={back}
              onChange={(e) => setBack(e.target.value)}
              placeholder="Randomised election timeouts and majority votes per term."
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" disabled={!front.trim() || save.isPending}>
              {draft?.id ? "Save" : "Add card"} <kbd className="ml-1 text-[10px] opacity-60">⌘↵</kbd>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
