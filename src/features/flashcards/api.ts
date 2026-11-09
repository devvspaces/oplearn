import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { create } from "zustand";
import type { Flashcard, Grade, NewFlashcard } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";

export const fk = {
  all: ["flashcards"] as const,
  lesson: (id: string | null) => ["flashcards", "list", id] as const,
  due: ["flashcards", "due"] as const,
};

export const useLessonCards = (lessonId: string | null) =>
  useQuery({ queryKey: fk.lesson(lessonId), queryFn: () => commands.listFlashcards(lessonId) });
export const useDueCards = () =>
  useQuery({ queryKey: fk.due, queryFn: () => commands.dueFlashcards(200), refetchInterval: 60_000 });

function useInvalidate() {
  const qc = useQueryClient();
  return () => {
    void qc.invalidateQueries({ queryKey: fk.all });
    void qc.invalidateQueries({ queryKey: ["study"] });
  };
}

export function useSaveCard() {
  const inv = useInvalidate();
  return useMutation({
    mutationFn: (v: { id?: string; card: NewFlashcard }) =>
      v.id ? commands.updateFlashcard(v.id, v.card.frontMd, v.card.backMd) : commands.createFlashcard(v.card),
    onSuccess: (_, v) => {
      inv();
      toast.success(v.id ? "Card updated" : "Flashcard added");
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}

export function useDeleteCard() {
  const inv = useInvalidate();
  return useMutation({
    mutationFn: (id: string) => commands.deleteFlashcard(id),
    onSuccess: inv,
    onError: (e) => toast.error(errorMessage(e)),
  });
}

export function useReview() {
  const inv = useInvalidate();
  return useMutation({
    mutationFn: (v: { id: string; grade: Grade; seconds: number }) =>
      commands.reviewFlashcard(v.id, v.grade, v.seconds),
    onSettled: inv,
    onError: (e) => toast.error(errorMessage(e)),
  });
}

/** Global "new / edit card" dialog state, so any panel can open it. */
interface CardDraft {
  id?: string;
  lessonId?: string | null;
  sourceSec?: number | null;
  sourceMaterialId?: string | null;
  sourcePage?: number | null;
  front: string;
  back: string;
}
export const useCardDialog = create<{
  draft: CardDraft | null;
  open: (d: CardDraft) => void;
  close: () => void;
}>()((set) => ({
  draft: null,
  open: (draft) => set({ draft }),
  close: () => set({ draft: null }),
}));

export const openCardEditor = (c: Flashcard) =>
  useCardDialog
    .getState()
    .open({
      id: c.id,
      lessonId: c.lessonId,
      sourceSec: c.sourceSec,
      sourceMaterialId: c.sourceMaterialId,
      sourcePage: c.sourcePage,
      front: c.frontMd,
      back: c.backMd,
    });
