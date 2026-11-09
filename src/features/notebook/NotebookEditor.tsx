import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Notebook, NotebookScope } from "@/bindings";
import { commands, errorMessage, qk, useNotebook } from "@/lib/api";
import { useAutosave } from "@/lib/autosave";
import { relativeTime } from "@/lib/time";
import { MarkdownEditor } from "@/components/MarkdownEditor";
import { Spinner } from "@/components/ui/misc";
import { Button } from "@/components/ui/button";
import { Sparkles } from "lucide-react";
import { useState } from "react";
import { AssistDialog } from "@/features/ai/AssistDialog";

/** Free-form Markdown notebook for a lesson or course, autosaved. */
export function NotebookEditor({
  scope,
  scopeId,
  placeholder,
}: {
  scope: NotebookScope;
  scopeId: string;
  placeholder: string;
}) {
  const qc = useQueryClient();
  const { data, isLoading } = useNotebook(scope, scopeId);
  const [summaryOpen, setSummaryOpen] = useState(false);
  const { schedule, flush } = useAutosave<string>((md) =>
    commands.saveNotebook(scope, scopeId, md).then(
      (nb) => qc.setQueryData<Notebook>(qk.notebook(scope, scopeId), nb),
      (e: unknown) => toast.error(errorMessage(e)),
    ),
  );
  if (isLoading || !data) return <Spinner />;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <MarkdownEditor
        key={`${scope}:${scopeId}`}
        className="min-h-0 flex-1 overflow-y-auto px-3 py-2 [&_.ProseMirror]:min-h-40"
        value={data.bodyMd}
        placeholder={placeholder}
        ariaLabel={`${scope} notebook`}
        onChange={schedule}
      />
      <div className="flex items-center gap-2 border-t px-3 py-1 text-[10px] text-muted-foreground">
        <span className="flex-1">{data.updatedAt ? `Saved ${relativeTime(data.updatedAt)}` : ""}</span>
        {scope === "lesson" && (
          <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => setSummaryOpen(true)}>
            <Sparkles /> Summarize with AI
          </Button>
        )}
      </div>
      {scope === "lesson" && (
        <AssistDialog
          open={summaryOpen}
          onOpenChange={setSummaryOpen}
          kind="summary"
          focus={{ lessonId: scopeId }}
          description="Claude writes an overview, key points with where each is taught, and a few self-check questions. It's added to the end of this notebook under an AI summary heading."
          saveLabel="Insert into notebook"
          onSave={async (ans) => {
            await flush();
            const sources = ans.sources.length
              ? `\n\nSources: ${ans.sources.map((s) => s.label).join(", ")}`
              : "";
            const nb = await commands.insertLessonSummary(scopeId, ans.markdown + sources);
            qc.setQueryData<Notebook>(qk.notebook(scope, scopeId), nb);
            toast.success("Summary added to the notebook");
          }}
        />
      )}
    </div>
  );
}
