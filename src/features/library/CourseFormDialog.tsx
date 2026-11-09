import { useEffect, useState } from "react";
import type { Course } from "@/bindings";
import { Button } from "@/components/ui/button";
import { Input, Label, Textarea } from "@/components/ui/input";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";

export interface CourseFormValues {
  title: string;
  /** Empty string clears the field on update. */
  description: string;
  instructor: string;
  sourceUrl: string;
  coverPath: string;
  tags: string[];
}

/** Shared create/edit form for course metadata. */
export function CourseFormDialog({
  open,
  onOpenChange,
  initial,
  title,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  initial?: Course;
  title: string;
  onSubmit: (v: CourseFormValues) => Promise<unknown>;
}) {
  const [f, setF] = useState({
    title: "",
    description: "",
    instructor: "",
    sourceUrl: "",
    coverPath: "",
    tags: "",
  });
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open)
      setF({
        title: initial?.title ?? "",
        description: initial?.description ?? "",
        instructor: initial?.instructor ?? "",
        sourceUrl: initial?.sourceUrl ?? "",
        coverPath: initial?.coverPath ?? "",
        tags: initial?.tags.join(", ") ?? "",
      });
  }, [open, initial]);
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
    setF((p) => ({ ...p, [k]: e.target.value }));

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      await onSubmit({
        title: f.title.trim(),
        description: f.description.trim(),
        instructor: f.instructor.trim(),
        sourceUrl: f.sourceUrl.trim(),
        coverPath: f.coverPath.trim(),
        tags: f.tags
          .split(",")
          .map((t) => t.trim())
          .filter(Boolean),
      });
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor="c-title">Title</Label>
            <Input id="c-title" autoFocus required value={f.title} onChange={set("title")} />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="c-desc">Description</Label>
            <Textarea id="c-desc" value={f.description} onChange={set("description")} />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-2">
              <Label htmlFor="c-inst">Instructor</Label>
              <Input id="c-inst" value={f.instructor} onChange={set("instructor")} />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="c-tags">Tags (comma separated)</Label>
              <Input id="c-tags" value={f.tags} onChange={set("tags")} />
            </div>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="c-src">Source URL</Label>
            <Input id="c-src" type="url" value={f.sourceUrl} onChange={set("sourceUrl")} />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="c-cover">Cover image URL</Label>
            <Input id="c-cover" value={f.coverPath} onChange={set("coverPath")} placeholder="https://…" />
          </div>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !f.title.trim()}>
              Save
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
