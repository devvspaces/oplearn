import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";
import type { YtSearchResult } from "@/bindings";
import { commands, errorMessage, useCourse, useCourses } from "@/lib/api";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input, Label } from "@/components/ui/input";

const NEW = "__new__";
const selectCls = "w-full rounded-md border bg-background px-2 py-1.5 text-sm";

/** Adds a search result video to a module of an existing course, or to a new course. */
export function AddToCourseDialog({ video, onClose }: { video: YtSearchResult | null; onClose: () => void }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const { data: courses } = useCourses();
  const [courseId, setCourseId] = useState<string>(NEW);
  const [moduleId, setModuleId] = useState<string>("");
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const { data: detail } = useCourse(courseId === NEW ? "" : courseId);
  useEffect(() => {
    if (!video) return;
    const active = (courses ?? []).filter((c) => c.course.status === "active");
    setCourseId(active[0]?.course.id ?? NEW);
    setTitle(video.channelTitle ? `${video.channelTitle}: collection` : "My collection");
  }, [video, courses]);
  useEffect(() => {
    const mods = detail?.modules ?? [];
    setModuleId(mods[mods.length - 1]?.module.id ?? "");
  }, [detail]);

  async function submit() {
    if (!video) return;
    setBusy(true);
    try {
      let cid = courseId;
      let mid = moduleId;
      if (courseId === NEW) {
        const c = await commands.createCourse({ title: title.trim() || "New course" });
        cid = c.id;
        mid = (await commands.createModule(c.id, "Lessons")).id;
      } else if (!mid) {
        mid = (await commands.createModule(cid, "Lessons")).id;
      }
      await commands.addVideoToModule(mid, `https://www.youtube.com/watch?v=${video.id}`);
      for (const k of [["courses"], ["course"]]) void qc.invalidateQueries({ queryKey: k });
      toast.success("Added to course", {
        action: {
          label: "Open",
          onClick: () => void navigate({ to: "/courses/$courseId", params: { courseId: cid } }),
        },
      });
      onClose();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={!!video} onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add to course</DialogTitle>
          <DialogDescription className="line-clamp-2">{video?.title}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3">
          <div className="grid gap-1">
            <Label htmlFor="atc-course">Course</Label>
            <select
              id="atc-course"
              className={selectCls}
              value={courseId}
              onChange={(e) => setCourseId(e.target.value)}
            >
              {(courses ?? []).map((c) => (
                <option key={c.course.id} value={c.course.id}>
                  {c.course.title}
                </option>
              ))}
              <option value={NEW}>+ New course…</option>
            </select>
          </div>
          {courseId === NEW ? (
            <div className="grid gap-1">
              <Label htmlFor="atc-title">New course title</Label>
              <Input id="atc-title" value={title} onChange={(e) => setTitle(e.target.value)} />
            </div>
          ) : (
            <div className="grid gap-1">
              <Label htmlFor="atc-module">Module</Label>
              <select
                id="atc-module"
                className={selectCls}
                value={moduleId}
                onChange={(e) => setModuleId(e.target.value)}
              >
                {(detail?.modules ?? []).map((m) => (
                  <option key={m.module.id} value={m.module.id}>
                    {m.module.title}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={() => void submit()} disabled={busy}>
            {busy ? "Adding…" : "Add"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
