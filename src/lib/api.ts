import { convertFileSrc } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { commands, type AppError, type NotebookScope } from "@/bindings";

export { commands };

export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

/** Image src for a remote URL or a local file inside the data dir. */
export function mediaSrc(pathOrUrl: string | null | undefined): string | undefined {
  if (!pathOrUrl) return undefined;
  if (/^https?:\/\//.test(pathOrUrl)) return pathOrUrl;
  return convertFileSrc(pathOrUrl);
}

export function videoThumb(videoId: string | null | undefined): string | undefined {
  return videoId ? `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg` : undefined;
}

export const qk = {
  courses: ["courses"] as const,
  course: (id: string) => ["course", id] as const,
  lesson: (id: string) => ["lesson", id] as const,
  annotations: (lessonId: string) => ["annotations", lessonId] as const,
  courseAnnotations: (courseId: string) => ["courseAnnotations", courseId] as const,
  transcript: (lessonId: string) => ["transcript", lessonId] as const,
  chapters: (lessonId: string) => ["chapters", lessonId] as const,
  notebook: (scope: NotebookScope, id: string) => ["notebook", scope, id] as const,
  appInfo: ["appInfo"] as const,
  ytdlpVersion: ["ytdlpVersion"] as const,
};

export const useCourses = () => useQuery({ queryKey: qk.courses, queryFn: commands.listCourses });
export const useCourse = (id: string) =>
  useQuery({ queryKey: qk.course(id), queryFn: () => commands.getCourseDetail(id), enabled: !!id });
export const useLessonContext = (id: string) =>
  useQuery({ queryKey: qk.lesson(id), queryFn: () => commands.getLessonContext(id) });
export const useAppInfo = () => useQuery({ queryKey: qk.appInfo, queryFn: commands.getAppInfo });

/** Mutation that invalidates library-wide queries and toasts failures. */
export function useLibraryMutation<TVars, TData>(
  fn: (v: TVars) => Promise<TData>,
  opts?: { success?: string },
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: qk.courses });
      void qc.invalidateQueries({ queryKey: ["course"] });
      void qc.invalidateQueries({ queryKey: ["lesson"] });
      void qc.invalidateQueries({ queryKey: ["study"] });
      if (opts?.success) toast.success(opts.success);
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}

export const useAnnotations = (lessonId: string) =>
  useQuery({ queryKey: qk.annotations(lessonId), queryFn: () => commands.listAnnotations(lessonId) });
export const useCourseAnnotations = (courseId: string) =>
  useQuery({
    queryKey: qk.courseAnnotations(courseId),
    queryFn: () => commands.listCourseAnnotations(courseId),
  });
export const useTranscript = (lessonId: string) =>
  useQuery({ queryKey: qk.transcript(lessonId), queryFn: () => commands.getTranscript(lessonId) });
export const useChapters = (lessonId: string) =>
  useQuery({ queryKey: qk.chapters(lessonId), queryFn: () => commands.getChapters(lessonId) });
export const useNotebook = (scope: NotebookScope, id: string) =>
  useQuery({
    queryKey: qk.notebook(scope, id),
    queryFn: () => commands.getNotebook(scope, id),
    staleTime: Infinity,
  });

/** Mutation for annotation changes: refreshes the lesson's list and course-wide views. */
export function useAnnotationMutation<TVars, TData>(lessonId: string, fn: (v: TVars) => Promise<TData>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: qk.annotations(lessonId) });
      void qc.invalidateQueries({ queryKey: ["courseAnnotations"] });
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}
