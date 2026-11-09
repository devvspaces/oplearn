import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { BookOpen, Download, ExternalLink, GraduationCap, Search as SearchIcon } from "lucide-react";
import type { ImportProgress, Provider, ProviderCard } from "@/bindings";
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
import { Input } from "@/components/ui/input";
import { Badge, EmptyState, Progress, Spinner } from "@/components/ui/misc";
import { formatBytes } from "@/features/downloads/useDownloads";

export type OpenProvider = Exclude<Provider, "youtube" | "mit_ocw">;

/** Per-provider wording; everything else is shared. */
export const PROVIDERS: Record<
  OpenProvider,
  { label: string; site: string; placeholder: string; link: RegExp; blurb: string; browse: boolean }
> = {
  nptel: {
    label: "NPTEL",
    site: "nptel.ac.in",
    placeholder: "Search NPTEL courses, or paste a course link (nptel.ac.in/courses/…)",
    link: /nptel\.ac\.in\/courses\/\d+/,
    blurb:
      "About 3,500 IIT and IISc courses. Imports bring the weekly lecture videos with English transcripts where NPTEL has them, plus the syllabus.",
    browse: false,
  },
  openstax: {
    label: "OpenStax",
    site: "openstax.org",
    placeholder: "Search OpenStax textbooks, or paste a book link",
    link: /openstax\.org\/(details\/)?books\//,
    blurb:
      "Free, peer-reviewed textbooks. Each section becomes a reading lesson with its figures and math, stored on your computer and searchable.",
    browse: true,
  },
  libretexts: {
    label: "LibreTexts",
    site: "libretexts.org",
    placeholder: "Search LibreTexts books, or paste a Commons book link",
    link: /commons\.libretexts\.org\/book\//,
    blurb:
      "About 4,000 open textbooks. Each page becomes a reading lesson with its own PDF, indexed for search and AI.",
    browse: false,
  },
  yale: {
    label: "Open Yale Courses",
    site: "oyc.yale.edu",
    placeholder: "Search Open Yale Courses, or paste a course link",
    link: /oyc\.yale\.edu\/[a-z-]+\/[a-z0-9-]+/,
    blurb:
      "Recorded Yale College courses. Imports bring lecture videos with chapters and Yale's full transcripts, plus handouts, problem sets and exams.",
    browse: true,
  },
};

const STAGE: Record<string, string> = {
  download: "Downloading",
  extract: "Unpacking",
  build: "Building lessons and modules",
  index: "Indexing PDFs for search and AI",
  done: "Done",
};

export function LicenseBadge({ license, url }: { license?: string | null; url?: string | null }) {
  if (!license)
    return (
      <Badge tone="muted" title="The provider doesn't state a license for this item">
        License: see source
      </Badge>
    );
  return (
    <Badge tone="success" title={url ?? license}>
      {license}
    </Badge>
  );
}

export function ProviderDiscover({ provider }: { provider: OpenProvider }) {
  const cfg = PROVIDERS[provider];
  const [text, setText] = useState("");
  const [query, setQuery] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const results = useQuery({
    queryKey: ["providerSearch", provider, query],
    queryFn: () => commands.providerSearch(provider, query ?? ""),
    enabled: query !== null,
    staleTime: 10 * 60_000,
    retry: false,
  });
  const isLink = cfg.link.test(text);

  return (
    <div className="pb-8 pt-4">
      <form
        className="mb-6 flex gap-2 rounded-xl border bg-card p-3"
        onSubmit={(e) => {
          e.preventDefault();
          const t = text.trim();
          if (!t) return;
          if (isLink) setPreview(t);
          else setQuery(t);
        }}
      >
        <div className="relative flex-1">
          <SearchIcon
            className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden
          />
          <Input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={cfg.placeholder}
            className="pl-8"
            aria-label={`Search ${cfg.label}`}
          />
        </div>
        <Button type="submit" disabled={!text.trim()}>
          {isLink ? "Open" : "Search"}
        </Button>
      </form>

      {query === null ? (
        <EmptyState icon={<GraduationCap className="size-8" />} title={`Courses from ${cfg.label}`}>
          <p>{cfg.blurb}</p>
          <p className="mt-1">The first search downloads the catalogue, which is then kept for a day.</p>
          {cfg.browse && (
            <Button variant="outline" size="sm" className="mt-3" onClick={() => setQuery("")}>
              <BookOpen /> Browse all
            </Button>
          )}
        </EmptyState>
      ) : results.isLoading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Spinner /> {query ? "Searching…" : "Loading the catalogue…"}
        </div>
      ) : results.isError ? (
        <p className="text-sm text-destructive">{errorMessage(results.error)}</p>
      ) : !results.data?.length ? (
        <p className="text-sm text-muted-foreground">Nothing on {cfg.label} matched.</p>
      ) : (
        <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3" aria-label={`${cfg.label} results`}>
          {results.data.map((c) => (
            <CourseCard key={c.id} c={c} onPreview={() => setPreview(c.id)} />
          ))}
        </ul>
      )}
      <PreviewDialog provider={provider} id={preview} onClose={() => setPreview(null)} />
    </div>
  );
}

function CourseCard({ c, onPreview }: { c: ProviderCard; onPreview: () => void }) {
  return (
    <li className="flex flex-col overflow-hidden rounded-xl border bg-card">
      {c.imageUrl && (
        <div className="flex aspect-video items-center justify-center bg-muted">
          <img src={c.imageUrl} alt="" loading="lazy" className="size-full object-contain p-2" />
        </div>
      )}
      <div className="flex flex-1 flex-col p-3">
        {c.subtitle && <p className="truncate text-[11px] font-medium text-muted-foreground">{c.subtitle}</p>}
        <p className="mt-0.5 line-clamp-2 text-sm font-medium">{c.title}</p>
        {c.description && <p className="mt-1 line-clamp-3 text-xs text-muted-foreground">{c.description}</p>}
        {c.tags.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {c.tags.slice(0, 3).map((t) => (
              <Badge key={t} tone="muted">
                {t}
              </Badge>
            ))}
          </div>
        )}
        <div className="mt-auto flex items-center gap-2 pt-3">
          <LicenseBadge license={c.license} url={c.licenseUrl} />
          <Button size="sm" variant="outline" className="ml-auto shrink-0" onClick={onPreview}>
            Preview & import
          </Button>
        </div>
      </div>
    </li>
  );
}

function PreviewDialog({
  provider,
  id,
  onClose,
}: {
  provider: OpenProvider;
  id: string | null;
  onClose: () => void;
}) {
  const cfg = PROVIDERS[provider];
  const qc = useQueryClient();
  const navigate = useNavigate();
  const q = useQuery({
    queryKey: ["providerPreview", provider, id],
    queryFn: () => commands.providerPreview(provider, id!),
    enabled: !!id,
    retry: false,
  });
  const [prog, setProg] = useState<ImportProgress | null>(null);
  const [withPdf, setWithPdf] = useState(false);
  const busy = !!prog && prog.stage !== "done";
  const p = q.data;

  async function importIt() {
    if (!id) return;
    const ch = new Channel<ImportProgress>();
    ch.onmessage = (m) => setProg(m);
    setProg({ stage: "download", done: 0, total: 0, message: "Starting…" });
    try {
      const r = await commands.providerImport(provider, p?.card.id ?? id, withPdf, ch);
      for (const k of [["courses"], ["course"]]) void qc.invalidateQueries({ queryKey: k });
      const parts = [`${r.lessons} lessons`];
      if (r.transcripts) parts.push(`${r.transcripts} transcripts`);
      if (r.materials) parts.push(`${r.materials} materials`);
      toast.success(`Imported ${parts.join(", ")}`, {
        description:
          [
            r.pdfsIndexed ? `${r.pdfsIndexed} PDFs searchable` : "",
            r.skipped ? `${r.skipped} items skipped` : "",
          ]
            .filter(Boolean)
            .join(" · ") || undefined,
      });
      setProg(null);
      onClose();
      void navigate({ to: "/courses/$courseId", params: { courseId: r.courseId } });
    } catch (e) {
      toast.error(errorMessage(e));
      setProg(null);
    }
  }

  const pct = prog && prog.total ? (100 * (prog.done ?? 0)) / (prog.total ?? 1) : 0;
  const bytes = (n: number | null | undefined) => (n ? formatBytes(n) : null);
  return (
    <Dialog open={!!id} onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="max-h-[90vh] max-w-2xl grid-rows-[auto_1fr_auto] overflow-hidden">
        <DialogHeader>
          <DialogTitle>{p?.card.title ?? `${cfg.label} course`}</DialogTitle>
          <DialogDescription>
            {p ? (p.card.subtitle ?? cfg.label) : "Loading course details…"}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-4 overflow-y-auto text-sm">
          {q.isLoading ? (
            <Spinner />
          ) : q.isError ? (
            <p className="text-destructive">{errorMessage(q.error)}</p>
          ) : p ? (
            <>
              <div className="flex flex-wrap items-center gap-2">
                <LicenseBadge license={p.card.license} url={p.card.licenseUrl} />
                <Badge tone="muted">{p.counts.lessons} lessons</Badge>
                {p.counts.videos > 0 && <Badge tone="muted">{p.counts.videos} videos</Badge>}
                {p.counts.transcripts > 0 && <Badge tone="muted">{p.counts.transcripts} transcripts</Badge>}
                {p.counts.pdfs > 0 && <Badge tone="muted">{p.counts.pdfs} PDFs</Badge>}
              </div>
              {p.card.description && (
                <p className="line-clamp-5 whitespace-pre-wrap text-muted-foreground">{p.card.description}</p>
              )}
              <ul className="list-disc space-y-1 pl-5 text-xs">
                {p.notes.map((n) => (
                  <li key={n}>{n}</li>
                ))}
                {bytes(p.estBytes) && <li>Download: about {bytes(p.estBytes)}.</li>}
              </ul>
              {p.pdfBytes != null && (
                <label className="flex items-center gap-2 text-xs">
                  <input type="checkbox" checked={withPdf} onChange={(e) => setWithPdf(e.target.checked)} />
                  Also download the full-book PDF{bytes(p.pdfBytes) ? ` (${bytes(p.pdfBytes)})` : ""}
                </label>
              )}
              <div>
                <p className="mb-1 text-xs font-medium text-muted-foreground">Outline</p>
                <ol className="space-y-1 rounded-lg border p-2 text-xs" aria-label="Course outline">
                  {p.outline.map((m, i) => (
                    <li key={i}>
                      <details>
                        <summary className="cursor-pointer">
                          {m.title} <span className="text-muted-foreground">({m.items.length})</span>
                        </summary>
                        <ul className="ml-4 mt-1 list-disc space-y-0.5 pl-3 text-muted-foreground">
                          {m.items.map((it, j) => (
                            <li key={j}>{it}</li>
                          ))}
                        </ul>
                      </details>
                    </li>
                  ))}
                </ol>
              </div>
              <p className="text-xs text-muted-foreground">
                Imported for your personal study, with links back to {cfg.site}.
              </p>
              {prog && (
                <div role="status" className="rounded-lg border p-3 text-xs">
                  <div className="mb-1 flex justify-between gap-2">
                    <span>{prog.message || STAGE[prog.stage]}</span>
                    <span className="shrink-0 tabular-nums text-muted-foreground">
                      {prog.total && prog.total > 10_000
                        ? `${formatBytes(prog.done ?? 0)} of ${formatBytes(prog.total)}`
                        : prog.total
                          ? `${Math.round(prog.done ?? 0)}/${Math.round(prog.total ?? 0)}`
                          : ""}
                    </span>
                  </div>
                  <Progress value={pct} label="Import progress" />
                </div>
              )}
            </>
          ) : null}
        </div>
        <DialogFooter>
          {p && (
            <Button variant="ghost" onClick={() => void openUrl(p.card.url)} disabled={busy}>
              <ExternalLink /> View on {cfg.label}
            </Button>
          )}
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Close
          </Button>
          <Button onClick={() => void importIt()} disabled={!p || busy || p.counts.lessons === 0}>
            {busy ? <Spinner /> : <Download />} {busy ? "Importing…" : "Import course"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
