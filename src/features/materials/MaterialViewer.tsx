import { useEffect, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Eye, EyeOff } from "lucide-react";
import type { Material } from "@/bindings";
import { commands } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Badge, Spinner } from "@/components/ui/misc";
import { KindIcon, kindLabel } from "./api";
import { PdfViewer, type SelectionAction } from "./PdfViewer";
import { cn } from "@/lib/utils";

/** Renders an imported page's HTML and routes its rewritten links (#material:, #page:). */
export function MaterialHtml({ id, pages }: { id: string; pages?: Material[] }) {
  const navigate = useNavigate();
  const { data: html, isLoading } = useQuery({
    queryKey: ["materialHtml", id],
    queryFn: () => commands.getMaterialHtml(id),
  });
  if (isLoading) return <Spinner />;
  return (
    <div
      className="prose-material text-sm leading-relaxed [&_a]:text-primary [&_a]:underline [&_h2]:mt-5 [&_h2]:text-base [&_h2]:font-semibold [&_h3]:mt-4 [&_h3]:font-semibold [&_li]:ml-5 [&_li]:list-disc [&_p]:my-2 [&_table]:my-3 [&_table]:w-full [&_table]:text-xs [&_td]:border [&_td]:px-2 [&_td]:py-1 [&_th]:border [&_th]:bg-muted [&_th]:px-2 [&_th]:py-1 [&_th]:text-left"
      onClick={(e) => {
        const a = (e.target as HTMLElement).closest("a");
        if (!a) return;
        const href = a.getAttribute("href") ?? "";
        e.preventDefault();
        if (href.startsWith("#material:")) {
          void navigate({ to: "/materials/$materialId", params: { materialId: href.slice(10) } });
        } else if (href.startsWith("#page:")) {
          const target = pages?.find((p) => p.section === href.slice(6));
          if (target) void navigate({ to: "/materials/$materialId", params: { materialId: target.id } });
        } else if (/^https?:/.test(href)) {
          void openUrl(href);
        }
      }}
      // Sanitised in Rust (scripts, event handlers and javascript: URLs removed; links rewritten).
      dangerouslySetInnerHTML={{ __html: html ?? "" }}
    />
  );
}

const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);

/** PDF (in-app viewer) or HTML page for a material, with its solution behind a reveal. */
export function MaterialViewer({
  material,
  solutions = [],
  page,
  query,
  onSelectText,
  className,
}: {
  material: Material;
  solutions?: Material[];
  page?: number | null;
  query?: string | null;
  onSelectText?: (action: SelectionAction, text: string, page: number, materialId: string) => void;
  className?: string;
}) {
  const [showSolution, setShowSolution] = useState(false);
  useEffect(() => setShowSolution(false), [material.id]);
  const shown = showSolution && solutions[0] ? solutions[0] : material;
  const isPdf = shown.fileType === "application/pdf" && !!shown.localPath;
  const { data: url } = useQuery({
    queryKey: ["materialUrl", shown.id],
    queryFn: () => commands.getMaterialUrl(shown.id),
    enabled: isPdf,
  });

  return (
    <div className={cn("flex min-h-0 flex-col overflow-hidden rounded-xl border bg-card", className)}>
      <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2 text-sm">
        <KindIcon kind={shown.kind} className="size-4 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate font-medium">{shown.title}</span>
        <Badge tone="muted">{kindLabel(shown.kind)}</Badge>
        {solutions.length > 0 && (
          <Button
            size="sm"
            variant={showSolution ? "secondary" : "outline"}
            onClick={() => setShowSolution((s) => !s)}
          >
            {showSolution ? <EyeOff /> : <Eye />} {showSolution ? "Hide solution" : "Show solution"}
          </Button>
        )}
        {isPdf && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void openPath(shown.localPath!)}
            title="Open in your PDF app"
          >
            {IS_MAC ? "Open in Preview" : "Open in PDF app"}
          </Button>
        )}
        {shown.sourceUrl && (
          <Button size="sm" variant="ghost" onClick={() => void openUrl(shown.sourceUrl!)}>
            <ExternalLink /> Open on OCW
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1">
        {isPdf ? (
          url ? (
            <PdfViewer
              key={shown.id}
              url={url}
              materialId={shown.id}
              page={showSolution ? 1 : page}
              query={showSolution ? null : query}
              onSelectText={onSelectText && ((a, t, p) => onSelectText(a, t, p, shown.id))}
            />
          ) : (
            <div className="p-6">
              <Spinner />
            </div>
          )
        ) : shown.hasHtml ? (
          <div className="h-full overflow-y-auto p-5">
            <MaterialHtml id={shown.id} />
          </div>
        ) : (
          <div className="flex flex-col items-center gap-3 p-10 text-center text-sm text-muted-foreground">
            <p>{shown.description ?? "This item isn't stored locally."}</p>
            {(shown.fileUrl || shown.sourceUrl) && (
              <Button variant="outline" onClick={() => void openUrl((shown.fileUrl || shown.sourceUrl)!)}>
                <ExternalLink /> Open online
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
