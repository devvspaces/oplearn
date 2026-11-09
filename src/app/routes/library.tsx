import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { ListVideo, Plus, PencilLine, Library as LibraryIcon } from "lucide-react";
import { commands, useCourses, useLibraryMutation } from "@/lib/api";
import { useUi } from "@/stores/ui";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { EmptyState, Spinner } from "@/components/ui/misc";
import { CourseCard } from "@/features/library/CourseCard";
import { ImportPlaylistDialog } from "@/features/library/ImportPlaylistDialog";
import { CourseFormDialog } from "@/features/library/CourseFormDialog";
import { cn } from "@/lib/utils";
import { FILTERS, filterCourses, type Sort } from "@/features/library/filter";

export const Route = createFileRoute("/library")({ component: LibraryPage });

function LibraryPage() {
  const { data, isLoading } = useCourses();
  const { libraryFilter: filter, setLibraryFilter } = useUi();
  const [sort, setSort] = useState<Sort>("recent");
  const [importOpen, setImportOpen] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const navigate = useNavigate();
  const create = useLibraryMutation(commands.createCourse, { success: "Course created" });

  const courses = filterCourses(data ?? [], filter, sort);

  return (
    <div className="mx-auto max-w-7xl p-8">
      <div className="flex items-center justify-between gap-4">
        <h1 className="text-2xl font-semibold">Library</h1>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button>
              <Plus /> New course
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={() => setImportOpen(true)}>
              <ListVideo /> From YouTube playlist…
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => setManualOpen(true)}>
              <PencilLine /> Blank course…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="mt-6 flex flex-wrap items-center justify-between gap-3">
        <div role="tablist" aria-label="Filter courses" className="flex gap-1 rounded-lg bg-muted p-1">
          {FILTERS.map(([key, label]) => (
            <button
              key={key}
              role="tab"
              aria-selected={filter === key}
              onClick={() => setLibraryFilter(key)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium text-muted-foreground transition-colors",
                filter === key && "bg-background text-foreground shadow-sm",
              )}
            >
              {label}
            </button>
          ))}
        </div>
        <label className="flex items-center gap-2 text-xs text-muted-foreground">
          Sort
          <select
            value={sort}
            onChange={(e) => setSort(e.target.value as Sort)}
            className="rounded-md border bg-background px-2 py-1 text-xs text-foreground"
          >
            <option value="recent">Recently studied</option>
            <option value="title">Title</option>
            <option value="progress">Progress</option>
          </select>
        </label>
      </div>

      <div className="mt-6">
        {isLoading ? (
          <Spinner />
        ) : courses.length === 0 ? (
          <EmptyState
            icon={<LibraryIcon className="size-8" />}
            title={data?.length ? "No courses match" : "Your library is empty"}
          >
            {!data?.length && (
              <>
                <p>Import a YouTube playlist to turn it into a course.</p>
                <Button className="mt-4" onClick={() => setImportOpen(true)}>
                  <ListVideo /> Import a playlist
                </Button>
              </>
            )}
          </EmptyState>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-4">
            {courses.map((c) => (
              <CourseCard key={c.course.id} summary={c} />
            ))}
          </div>
        )}
      </div>

      <ImportPlaylistDialog open={importOpen} onOpenChange={setImportOpen} />
      <CourseFormDialog
        open={manualOpen}
        onOpenChange={setManualOpen}
        title="New course"
        onSubmit={async (v) => {
          const c = await create.mutateAsync({
            ...v,
            description: v.description || null,
            instructor: v.instructor || null,
            sourceUrl: v.sourceUrl || null,
            coverPath: v.coverPath || null,
          });
          void navigate({ to: "/courses/$courseId", params: { courseId: c.id } });
        }}
      />
    </div>
  );
}
