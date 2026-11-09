import { useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, FileText, KeyRound } from "lucide-react";
import type { Material, ProviderMeta } from "@/bindings";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge, Spinner } from "@/components/ui/misc";
import { kindLabel, useCourseInfo, useCourseMaterials } from "./api";
import { MaterialHtml } from "./MaterialViewer";
import { cn } from "@/lib/utils";

/** Provider courses: metadata, attribution, syllabus and the other imported pages. */
export function AboutPanel({ courseId }: { courseId: string }) {
  const { data: info, isLoading } = useCourseInfo(courseId);
  const [pageId, setPageId] = useState<string | null>(null);
  if (isLoading || !info) return <Spinner />;
  const m = info.meta;
  const pm = info.providerMeta;
  const current = pageId ?? info.syllabusId ?? info.pages[0]?.id ?? null;
  return (
    <div className="grid gap-6 md:grid-cols-[16rem_1fr]">
      <aside className="space-y-4 text-sm">
        {m && (
          <dl className="space-y-2">
            {m.courseNumber && (
              <div>
                <dt className="text-xs text-muted-foreground">Course</dt>
                <dd>
                  {m.courseNumber}
                  {m.extraNumbers ? ` / ${m.extraNumbers}` : ""}
                </dd>
              </div>
            )}
            {m.term && (
              <div>
                <dt className="text-xs text-muted-foreground">As taught in</dt>
                <dd>{m.term}</dd>
              </div>
            )}
            {m.level.length > 0 && (
              <div>
                <dt className="text-xs text-muted-foreground">Level</dt>
                <dd>{m.level.join(", ")}</dd>
              </div>
            )}
            {m.instructors.length > 0 && (
              <div>
                <dt className="text-xs text-muted-foreground">Instructors</dt>
                <dd>
                  <ul>
                    {m.instructors.map((i) => (
                      <li key={i}>{i}</li>
                    ))}
                  </ul>
                </dd>
              </div>
            )}
            {m.topics.length > 0 && (
              <div>
                <dt className="text-xs text-muted-foreground">Topics</dt>
                <dd className="flex flex-wrap gap-1">
                  {m.topics.map((t) => (
                    <Badge key={t} tone="muted">
                      {t.split(" > ").pop()}
                    </Badge>
                  ))}
                </dd>
              </div>
            )}
          </dl>
        )}
        {pm && <ProviderFacts pm={pm} />}
        {info.pages.length > 0 && (
          <nav aria-label="Course pages" className="space-y-0.5">
            {info.pages.map((p) => (
              <button
                key={p.id}
                onClick={() => setPageId(p.id)}
                className={cn(
                  "block w-full rounded-md px-2 py-1 text-left hover:bg-accent",
                  current === p.id && "bg-accent font-medium",
                )}
              >
                {p.title}
              </button>
            ))}
          </nav>
        )}
        {m && (
          <div className="space-y-2 rounded-lg border p-3 text-xs text-muted-foreground">
            <p>
              <KeyRound className="mr-1 inline size-3.5" />
              MIT OpenCourseWare, licensed CC BY-NC-SA 4.0. Imported for personal study.
            </p>
            <Button size="sm" variant="outline" className="w-full" onClick={() => void openUrl(m.url)}>
              <ExternalLink /> Open on OCW
            </Button>
          </div>
        )}
        {pm && <ProviderLicense pm={pm} />}
      </aside>
      <div className="min-w-0 rounded-xl border bg-card p-5">
        {current ? (
          <MaterialHtml key={current} id={current} pages={info.pages} />
        ) : pm?.descriptionHtml ? (
          // Sanitised in Rust like every imported page.
          <div className="text-sm leading-relaxed" dangerouslySetInnerHTML={{ __html: pm.descriptionHtml }} />
        ) : (
          <p className="text-sm text-muted-foreground">{m?.description ?? "No pages were imported."}</p>
        )}
      </div>
    </div>
  );
}

function ProviderFacts({ pm }: { pm: ProviderMeta }) {
  return (
    <dl className="space-y-2">
      {pm.institution && (
        <div>
          <dt className="text-xs text-muted-foreground">From</dt>
          <dd>{pm.institution}</dd>
        </div>
      )}
      {pm.instructors.length > 0 && (
        <div>
          <dt className="text-xs text-muted-foreground">
            {pm.instructors.length > 1 ? "Authors" : "Instructor"}
          </dt>
          <dd>
            <ul>
              {pm.instructors.map((i) => (
                <li key={i}>{i}</li>
              ))}
            </ul>
          </dd>
        </div>
      )}
      {pm.facts.map((f) => (
        <div key={f.label}>
          <dt className="text-xs text-muted-foreground">{f.label}</dt>
          <dd>{f.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function ProviderLicense({ pm }: { pm: ProviderMeta }) {
  return (
    <div className="space-y-2 rounded-lg border p-3 text-xs text-muted-foreground">
      <p>
        <KeyRound className="mr-1 inline size-3.5" />
        {pm.providerLabel}
        {pm.license ? (
          <>
            , licensed{" "}
            {pm.licenseUrl ? (
              <button className="underline" onClick={() => void openUrl(pm.licenseUrl!)}>
                {pm.license}
              </button>
            ) : (
              pm.license
            )}
          </>
        ) : (
          "; no license is stated, so check the source before reusing it"
        )}
        . Imported for personal study.
      </p>
      {pm.attribution && <p>{pm.attribution}</p>}
      <Button size="sm" variant="outline" className="w-full" onClick={() => void openUrl(pm.url)}>
        <ExternalLink /> Open on {pm.providerLabel}
      </Button>
    </div>
  );
}

const GROUP_ORDER = [
  "lecture_notes",
  "reading",
  "assignment",
  "recitation",
  "project",
  "exam",
  "video",
  "other",
];

/** All imported files grouped by kind, with solutions beside what they solve. */
export function MaterialsPanel({ courseId }: { courseId: string }) {
  const { data, isLoading } = useCourseMaterials(courseId);
  const [q, setQ] = useState("");
  const groups = useMemo(() => {
    const all = (data ?? []).filter((m) => !m.hasHtml && m.kind !== "video");
    const solutions = new Map<string, Material[]>();
    for (const m of all)
      if (m.solutionOf) solutions.set(m.solutionOf, [...(solutions.get(m.solutionOf) ?? []), m]);
    const needle = q.trim().toLowerCase();
    const base = all.filter(
      (m) => !m.solutionOf && (!needle || `${m.title} ${m.description ?? ""}`.toLowerCase().includes(needle)),
    );
    const by = new Map<string, Material[]>();
    for (const m of base) by.set(m.kind, [...(by.get(m.kind) ?? []), m]);
    return {
      solutions,
      list: [...by.entries()].sort(
        ([a], [b]) => ((GROUP_ORDER.indexOf(a) + 99) % 99) - ((GROUP_ORDER.indexOf(b) + 99) % 99),
      ),
    };
  }, [data, q]);

  if (isLoading) return <Spinner />;
  if (!data?.length)
    return <p className="text-sm text-muted-foreground">This course has no imported materials.</p>;
  return (
    <div className="space-y-5">
      <Input
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Filter materials…"
        className="max-w-sm"
      />
      {groups.list.map(([kind, items]) => (
        <section key={kind}>
          <h3 className="mb-2 text-sm font-semibold">
            {kindLabel(kind)}s <span className="font-normal text-muted-foreground">({items.length})</span>
          </h3>
          <ul className="divide-y rounded-xl border bg-card">
            {items.map((m) => {
              const sols = groups.solutions.get(m.id) ?? [];
              return (
                <li key={m.id} className="flex items-center gap-3 px-3 py-2 text-sm">
                  <FileText className="size-4 shrink-0 text-muted-foreground" aria-hidden />
                  <div className="min-w-0 flex-1">
                    <Link
                      to="/materials/$materialId"
                      params={{ materialId: m.id }}
                      className="truncate hover:underline"
                    >
                      {m.title}
                    </Link>
                    {m.description && (
                      <p className="truncate text-xs text-muted-foreground">{m.description}</p>
                    )}
                  </div>
                  {sols.length > 0 && <Badge tone="success">Solution</Badge>}
                  {m.pageCount ? (
                    <span className="text-xs tabular-nums text-muted-foreground">{m.pageCount} p.</span>
                  ) : null}
                  {m.lessonId && (
                    <Link
                      to="/lessons/$lessonId"
                      params={{ lessonId: m.lessonId }}
                      className="text-xs text-primary hover:underline"
                    >
                      Lesson
                    </Link>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
