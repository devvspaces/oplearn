import { createElement } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  BookOpen,
  CalendarDays,
  ClipboardList,
  FileCheck,
  FileText,
  FolderKanban,
  NotebookText,
  PlayCircle,
  Users,
  type LucideIcon,
} from "lucide-react";
import { commands } from "@/lib/api";

export const useMaterial = (id: string) =>
  useQuery({ queryKey: ["material", id], queryFn: () => commands.getMaterial(id), enabled: !!id });
export const useLessonMaterials = (lessonId: string | null) =>
  useQuery({
    queryKey: ["lessonMaterials", lessonId],
    queryFn: () => commands.listLessonMaterials(lessonId!),
    enabled: !!lessonId,
  });
export const useCourseMaterials = (courseId: string) =>
  useQuery({ queryKey: ["materials", courseId], queryFn: () => commands.listMaterials(courseId) });
export const useCourseInfo = (courseId: string) =>
  useQuery({ queryKey: ["courseInfo", courseId], queryFn: () => commands.getCourseInfo(courseId) });

export const KIND_LABEL: Record<string, string> = {
  video: "Video",
  lecture_notes: "Lecture notes",
  reading: "Reading",
  assignment: "Problem set",
  exam: "Exam",
  recitation: "Recitation",
  project: "Project",
  other: "Other",
  syllabus: "Syllabus",
  calendar: "Calendar",
  page: "Page",
  transcript: "Transcript",
};

export const kindLabel = (k: string) =>
  k.endsWith("_solution")
    ? `${KIND_LABEL[k.replace("_solution", "")] ?? "Item"} solution`
    : (KIND_LABEL[k] ?? k);

const KIND_ICON: Record<string, LucideIcon> = {
  video: PlayCircle,
  lecture_notes: NotebookText,
  reading: BookOpen,
  assignment: ClipboardList,
  exam: FileCheck,
  recitation: Users,
  project: FolderKanban,
  calendar: CalendarDays,
};

/** Icon for a material kind (solutions use their base kind's icon). */
export const kindIcon = (k: string | null | undefined): LucideIcon =>
  KIND_ICON[(k ?? "").replace("_solution", "")] ?? FileText;

export function KindIcon({ kind, className }: { kind: string | null | undefined; className?: string }) {
  return createElement(kindIcon(kind), { className, "aria-hidden": true });
}
