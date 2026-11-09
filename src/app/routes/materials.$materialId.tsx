import { Link, createFileRoute } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Spinner } from "@/components/ui/misc";
import { commands, errorMessage, qk } from "@/lib/api";
import { useCardDialog } from "@/features/flashcards/api";
import { MaterialViewer } from "@/features/materials/MaterialViewer";
import { useCourseInfo, useCourseMaterials, useMaterial } from "@/features/materials/api";

export const Route = createFileRoute("/materials/$materialId")({
  validateSearch: (s: Record<string, unknown>): { page?: number; q?: string } => {
    const p = Number(s.page);
    return {
      ...(Number.isFinite(p) && p >= 1 ? { page: Math.floor(p) } : {}),
      ...(typeof s.q === "string" && s.q ? { q: s.q } : {}),
    };
  },
  component: MaterialPage,
});

function MaterialPage() {
  const { materialId } = Route.useParams();
  const { page, q } = Route.useSearch();
  const qc = useQueryClient();
  const { data: m, isLoading } = useMaterial(materialId);
  const { data: all } = useCourseMaterials(m?.courseId ?? "");
  useCourseInfo(m?.courseId ?? "");
  if (isLoading)
    return (
      <div className="p-8">
        <Spinner />
      </div>
    );
  if (!m) return <div className="p-8 text-destructive">Not found.</div>;
  const solutions = (all ?? []).filter((x) => x.solutionOf === m.id);
  return (
    <div className="mx-auto flex h-full max-w-6xl flex-col gap-3 p-6">
      <div className="flex items-center gap-3 text-sm text-muted-foreground">
        <Link
          to="/courses/$courseId"
          params={{ courseId: m.courseId }}
          className="inline-flex items-center gap-1 hover:text-foreground"
        >
          <ArrowLeft className="size-4" /> Back to course
        </Link>
        {m.lessonId && (
          <Link to="/lessons/$lessonId" params={{ lessonId: m.lessonId }} className="hover:text-foreground">
            Open as lesson (notes & progress)
          </Link>
        )}
      </div>
      <MaterialViewer
        material={m}
        solutions={solutions}
        page={page}
        query={q}
        className="min-h-0 flex-1"
        onSelectText={
          m.lessonId
            ? (action, text, p, materialId) => {
                const lessonId = m.lessonId!;
                if (action === "card") {
                  useCardDialog
                    .getState()
                    .open({ lessonId, sourceMaterialId: materialId, sourcePage: p, front: "", back: text });
                  return;
                }
                // Notes on a document live with its lesson (e.g. lecture notes beside a video).
                commands
                  .createAnnotation({ lessonId, type: action, materialId, page: p, bodyMd: `> ${text}\n\n` })
                  .then(() => {
                    void qc.invalidateQueries({ queryKey: qk.annotations(lessonId) });
                    void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
                    toast.success(`${action === "note" ? "Note" : "Question"} saved on page ${p}`, {
                      description: "It's in the lesson's Notes tab.",
                    });
                  })
                  .catch((e: unknown) => toast.error(errorMessage(e)));
              }
            : undefined
        }
      />
    </div>
  );
}
