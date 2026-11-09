import { useState } from "react";
import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Eye, Info, ListVideo, Plus, Search as SearchIcon, UserRound } from "lucide-react";
import type { DurationFilter, SearchKind, UploadFilter, YtSearchQuery, YtSearchResult } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge, EmptyState, Spinner } from "@/components/ui/misc";
import { formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import { AddToCourseDialog } from "@/features/youtube/AddToCourseDialog";
import { PlaylistPreviewDialog } from "@/features/youtube/PlaylistPreviewDialog";
import { OcwDiscover } from "@/features/ocw/OcwDiscover";
import { ProviderDiscover } from "@/features/providers/ProviderDiscover";

export const Route = createFileRoute("/search")({
  validateSearch: (s: Record<string, unknown>): { q?: string } => (typeof s.q === "string" ? { q: s.q } : {}),
  component: DiscoverPage,
});

const SOURCES = [
  ["youtube", "YouTube"],
  ["ocw", "MIT OCW"],
  ["nptel", "NPTEL"],
  ["openstax", "OpenStax"],
  ["libretexts", "LibreTexts"],
  ["yale", "Open Yale"],
] as const;
type Source = (typeof SOURCES)[number][0];
const isSource = (s: string | null): s is Source => SOURCES.some(([k]) => k === s);

function DiscoverPage() {
  const [provider, setProvider] = useState<Source>(() => {
    try {
      const saved = localStorage.getItem("discover.provider");
      return isSource(saved) ? saved : "youtube";
    } catch {
      return "youtube";
    }
  });
  const pick = (p: Source) => {
    setProvider(p);
    try {
      localStorage.setItem("discover.provider", p);
    } catch {
      /* per-viewer convenience only */
    }
  };
  return (
    <div className="mx-auto max-w-6xl px-8 pt-8">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="mr-auto text-2xl font-semibold">Discover</h1>
        <div role="radiogroup" aria-label="Provider" className="flex flex-wrap gap-1 rounded-lg bg-muted p-1">
          {SOURCES.map(([k, label]) => (
            <button
              key={k}
              role="radio"
              aria-checked={provider === k}
              onClick={() => pick(k)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium text-muted-foreground",
                provider === k && "bg-background text-foreground shadow-sm",
              )}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      {provider === "youtube" ? (
        <SearchPage />
      ) : provider === "ocw" ? (
        <OcwDiscover />
      ) : (
        <ProviderDiscover key={provider} provider={provider} />
      )}
    </div>
  );
}

const selectCls = "rounded-md border bg-background px-2 py-1 text-xs";

function compact(n: number | null | undefined) {
  if (n == null) return "";
  return new Intl.NumberFormat(undefined, { notation: "compact" }).format(n) + " views";
}

function SearchPage() {
  const { q: initial } = Route.useSearch();
  const [text, setText] = useState(initial ?? "");
  const [kind, setKind] = useState<SearchKind>("video");
  const [ccOnly, setCcOnly] = useState(false);
  const [duration, setDuration] = useState<DurationFilter | "">("");
  const [upload, setUpload] = useState<UploadFilter | "">("");
  const [submitted, setSubmitted] = useState<YtSearchQuery | null>(
    initial ? { query: initial, kind: "video", ccOnly: false } : null,
  );
  const [tab, setTab] = useState<"search" | "mine">("search");
  const [adding, setAdding] = useState<YtSearchResult | null>(null);

  const results = useQuery({
    queryKey: ["ytSearch", submitted],
    queryFn: () => commands.searchYoutube(submitted!),
    enabled: !!submitted,
    staleTime: 5 * 60_000,
    retry: false,
  });
  const account = useQuery({ queryKey: ["googleAccount"], queryFn: commands.getGoogleAccount });
  const mine = useQuery({
    queryKey: ["myPlaylists"],
    queryFn: commands.listMyPlaylists,
    enabled: tab === "mine" && !!account.data?.signedIn,
    retry: false,
  });

  const run = () => {
    if (!text.trim()) return;
    setTab("search");
    setSubmitted({
      query: text.trim(),
      kind,
      ccOnly: kind === "video" && ccOnly,
      duration: kind === "video" && duration ? duration : null,
      upload: upload || null,
    });
  };

  return (
    <div className="pb-8 pt-4">
      <header className="mb-4 flex flex-wrap items-center gap-3">
        <span className="mr-auto text-sm text-muted-foreground">
          Search YouTube for lectures and playlists
        </span>
        <div className="flex gap-1 rounded-lg bg-muted p-1" role="tablist">
          {(
            [
              ["search", "Search"],
              ["mine", "My playlists"],
            ] as const
          ).map(([k, label]) => (
            <button
              key={k}
              role="tab"
              aria-selected={tab === k}
              onClick={() => setTab(k)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium text-muted-foreground",
                tab === k && "bg-background text-foreground shadow-sm",
              )}
            >
              {label}
            </button>
          ))}
        </div>
      </header>

      <form
        className="mb-6 rounded-xl border bg-card p-3"
        onSubmit={(e) => {
          e.preventDefault();
          run();
        }}
      >
        <div className="flex gap-2">
          <div className="relative flex-1">
            <SearchIcon
              className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
              aria-hidden
            />
            <Input
              autoFocus
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="e.g. distributed systems lecture"
              className="pl-8"
              aria-label="Search YouTube"
            />
          </div>
          <Button type="submit" disabled={!text.trim() || results.isFetching}>
            Search
          </Button>
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-4 text-xs">
          <div role="radiogroup" aria-label="Result type" className="flex gap-1 rounded-lg bg-muted p-1">
            {(["video", "playlist"] as const).map((k) => (
              <button
                key={k}
                type="button"
                role="radio"
                aria-checked={kind === k}
                onClick={() => setKind(k)}
                className={cn(
                  "rounded-md px-2.5 py-0.5 capitalize text-muted-foreground",
                  kind === k && "bg-background text-foreground shadow-sm",
                )}
              >
                {k}s
              </button>
            ))}
          </div>
          <label className={cn("flex items-center gap-1.5", kind !== "video" && "opacity-50")}>
            <input
              type="checkbox"
              checked={ccOnly}
              disabled={kind !== "video"}
              onChange={(e) => setCcOnly(e.target.checked)}
            />
            Creative Commons only
          </label>
          <label className={cn("flex items-center gap-1.5", kind !== "video" && "opacity-50")}>
            Length
            <select
              className={selectCls}
              value={duration}
              disabled={kind !== "video"}
              onChange={(e) => setDuration(e.target.value as DurationFilter | "")}
            >
              <option value="">Any</option>
              <option value="short">Under 4 min</option>
              <option value="medium">4–20 min</option>
              <option value="long">Over 20 min</option>
            </select>
          </label>
          <label className="flex items-center gap-1.5">
            Uploaded
            <select
              className={selectCls}
              value={upload}
              onChange={(e) => setUpload(e.target.value as UploadFilter | "")}
            >
              <option value="">Any time</option>
              <option value="week">This week</option>
              <option value="month">This month</option>
              <option value="year">This year</option>
            </select>
          </label>
        </div>
      </form>

      {tab === "mine" ? (
        <MyPlaylists
          signedIn={!!account.data?.signedIn}
          configured={!!account.data?.configured}
          query={mine}
        />
      ) : !submitted ? (
        <EmptyState icon={<SearchIcon className="size-8" />} title="Search for lectures and courses">
          <p>Tick "Creative Commons only" to find videos you're free to download and reuse.</p>
          <p className="mt-1">
            Khan Academy and other channels' playlists can be imported here too: search for them, or paste a
            playlist link in Library → New course → From YouTube playlist.
          </p>
        </EmptyState>
      ) : results.isLoading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Spinner /> Searching…
        </div>
      ) : results.isError ? (
        <p className="text-sm text-destructive">{errorMessage(results.error)}</p>
      ) : (
        <>
          {results.data?.notice && (
            <p
              role="status"
              className="mb-4 flex items-start gap-2 rounded-lg border bg-muted/50 p-3 text-xs text-muted-foreground"
            >
              <Info className="mt-px size-4 shrink-0" /> {results.data.notice}{" "}
              <Link to="/settings" className="text-primary hover:underline">
                Settings
              </Link>
            </p>
          )}
          {results.data?.results.length === 0 ? (
            <p className="text-sm text-muted-foreground">No results.</p>
          ) : (
            <ResultGrid results={results.data?.results ?? []} onAdd={setAdding} />
          )}
          {results.data?.cached && (
            <p className="mt-4 text-center text-[11px] text-muted-foreground">
              Cached result (refreshes after 24 h, no quota used)
            </p>
          )}
        </>
      )}
      <AddToCourseDialog video={adding} onClose={() => setAdding(null)} />
    </div>
  );
}

function useImportPlaylist() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  return useMutation({
    mutationFn: (id: string) => commands.importPlaylist(`https://www.youtube.com/playlist?list=${id}`),
    onSuccess: (r) => {
      void qc.invalidateQueries({ queryKey: ["courses"] });
      toast.success(`Created a course with ${r.added} lessons`);
      void navigate({ to: "/courses/$courseId", params: { courseId: r.courseId } });
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}

function ResultGrid({ results, onAdd }: { results: YtSearchResult[]; onAdd: (r: YtSearchResult) => void }) {
  const importPl = useImportPlaylist();
  const [preview, setPreview] = useState<string | null>(null);
  return (
    <>
      <PlaylistPreviewDialog
        playlistId={preview}
        onClose={() => setPreview(null)}
        onCreate={(id) => {
          setPreview(null);
          importPl.mutate(id);
        }}
      />
      <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {results.map((r) => (
          <li key={`${r.kind}-${r.id}`} className="flex flex-col overflow-hidden rounded-xl border bg-card">
            <div className="relative aspect-video bg-muted">
              {r.thumbnailUrl && (
                <img src={r.thumbnailUrl} alt="" loading="lazy" className="size-full object-cover" />
              )}
              {r.durationSec != null && (
                <span className="absolute bottom-1.5 right-1.5 rounded bg-black/80 px-1.5 py-0.5 text-[11px] tabular-nums text-white">
                  {formatDuration(r.durationSec)}
                </span>
              )}
              {r.kind === "playlist" && (
                <span className="absolute bottom-1.5 right-1.5 flex items-center gap-1 rounded bg-black/80 px-1.5 py-0.5 text-[11px] text-white">
                  <ListVideo className="size-3" />{" "}
                  {r.itemCount != null ? `${r.itemCount} videos` : "Playlist"}
                </span>
              )}
            </div>
            <div className="flex flex-1 flex-col p-3">
              <p className="line-clamp-2 text-sm font-medium" title={r.title}>
                {r.title}
              </p>
              <p className="mt-1 truncate text-xs text-muted-foreground">
                {[
                  r.channelTitle,
                  compact(r.viewCount),
                  r.publishedAt ? new Date(r.publishedAt).getFullYear() : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </p>
              <div className="mt-auto flex items-center gap-2 pt-3">
                {r.license === "creativeCommon" && (
                  <Badge tone="success" title="Creative Commons licensed">
                    CC
                  </Badge>
                )}
                {r.kind === "video" ? (
                  <Button size="sm" variant="outline" className="ml-auto" onClick={() => onAdd(r)}>
                    <Plus /> Add to course
                  </Button>
                ) : (
                  <>
                    <Button size="sm" variant="ghost" className="ml-auto" onClick={() => setPreview(r.id)}>
                      <Eye /> Preview
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={importPl.isPending}
                      onClick={() => importPl.mutate(r.id)}
                    >
                      <Plus />{" "}
                      {importPl.isPending && importPl.variables === r.id ? "Creating…" : "Create course"}
                    </Button>
                  </>
                )}
              </div>
            </div>
          </li>
        ))}
      </ul>
    </>
  );
}

function MyPlaylists({
  signedIn,
  configured,
  query,
}: {
  signedIn: boolean;
  configured: boolean;
  query: ReturnType<typeof useQuery<YtSearchResult[]>>;
}) {
  if (!signedIn)
    return (
      <EmptyState icon={<UserRound className="size-8" />} title="Sign in to import your own playlists">
        {configured ? "Sign in with Google" : "Add your Google OAuth client and sign in"} under{" "}
        <Link to="/settings" className="text-primary hover:underline">
          Settings → Accounts
        </Link>
        . Watch Later and history aren't available through YouTube's API.
      </EmptyState>
    );
  if (query.isLoading) return <Spinner />;
  if (query.isError) return <p className="text-sm text-destructive">{errorMessage(query.error)}</p>;
  if (!query.data?.length)
    return <p className="text-sm text-muted-foreground">Your account has no playlists.</p>;
  return <ResultGrid results={query.data} onAdd={() => undefined} />;
}
