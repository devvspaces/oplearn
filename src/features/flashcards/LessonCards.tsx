import { useState } from "react";
import { Layers, Pencil, Plus, Sparkles, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/misc";
import { GenerateCardsDialog } from "./GenerateCardsDialog";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/misc";
import { player } from "@/features/player/store";
import { formatTimestamp } from "@/lib/time";
import { pdfNav } from "@/features/materials/pdf-nav";
import { openCardEditor, useCardDialog, useDeleteCard, useLessonCards } from "./api";

/** Lesson side-panel tab: this lesson's flashcards. */
export function LessonCards({ lessonId, isVideo }: { lessonId: string; isVideo: boolean }) {
  const { data, isLoading } = useLessonCards(lessonId);
  const del = useDeleteCard();
  const [gen, setGen] = useState(false);
  const add = () => {
    const doc = isVideo ? null : pdfNav.current();
    useCardDialog.getState().open({
      lessonId,
      sourceSec: isVideo ? Math.round(player.time() * 10) / 10 : null,
      sourceMaterialId: doc?.materialId,
      sourcePage: doc?.page,
      front: "",
      back: "",
    });
  };
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between border-b px-3 py-2">
        <span className="text-xs text-muted-foreground">{data?.length ?? 0} cards</span>
        <div className="flex gap-1.5">
          <Button size="sm" variant="outline" onClick={() => setGen(true)}>
            <Sparkles /> Generate with AI
          </Button>
          <Button size="sm" variant="outline" onClick={add}>
            <Plus /> New card
          </Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {isLoading ? (
          <Spinner />
        ) : !data?.length ? (
          <div className="flex flex-col items-center gap-2 p-8 text-center text-xs text-muted-foreground">
            <Layers className="size-6" />
            <p>No flashcards yet. Add one here, from a note's menu, or by selecting transcript text.</p>
          </div>
        ) : (
          <ul className="space-y-2">
            {data.map((c) => (
              <li key={c.id} className="group rounded-lg border bg-background/50 p-2 text-sm">
                <div className="flex items-start gap-2">
                  <div className="min-w-0 flex-1">
                    <p className="font-medium">{c.frontMd}</p>
                    <p className="mt-1 line-clamp-3 whitespace-pre-wrap text-xs text-muted-foreground">
                      {c.backMd}
                    </p>
                  </div>
                  <div className="flex opacity-0 group-hover:opacity-100 focus-within:opacity-100">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label="Edit card"
                      onClick={() => openCardEditor(c)}
                    >
                      <Pencil />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label="Delete card"
                      onClick={() => del.mutate(c.id)}
                    >
                      <Trash2 />
                    </Button>
                  </div>
                </div>
                <div className="mt-1 flex gap-2 text-[10px] text-muted-foreground">
                  {c.sourceSec != null ? (
                    <button
                      className="font-mono text-primary hover:underline"
                      onClick={() => player.seek(c.sourceSec ?? 0)}
                    >
                      {formatTimestamp(c.sourceSec)}
                    </button>
                  ) : c.sourcePage != null && c.sourceMaterialId ? (
                    <button
                      className="font-mono text-primary hover:underline"
                      title="Go to this page"
                      onClick={() => pdfNav.goTo(c.sourceMaterialId!, c.sourcePage!)}
                    >
                      p.{c.sourcePage}
                    </button>
                  ) : null}
                  <span>due {new Date(c.dueAt).toLocaleDateString()}</span>
                  {c.aiGenerated && <Badge tone="primary">AI</Badge>}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
      <GenerateCardsDialog lessonId={lessonId} open={gen} onOpenChange={setGen} />
    </div>
  );
}
