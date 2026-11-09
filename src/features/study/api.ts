import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { events, type StudySettings, type Target } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { usePlayer } from "@/features/player/store";

export const sk = {
  all: ["study"] as const,
  today: ["study", "today"] as const,
  settings: ["study", "settings"] as const,
  plan: (courseId: string) => ["study", "plan", courseId] as const,
  activity: (days: number) => ["study", "activity", days] as const,
  review: (date: string | null) => ["study", "review", date] as const,
  planned: (from: string, to: string) => ["study", "planned", from, to] as const,
};

export const useToday = () =>
  useQuery({ queryKey: sk.today, queryFn: commands.getToday, refetchInterval: 60_000 });
export const useStudySettings = () => useQuery({ queryKey: sk.settings, queryFn: commands.getStudySettings });
export const useCoursePlan = (courseId: string) =>
  useQuery({ queryKey: sk.plan(courseId), queryFn: () => commands.getCoursePlan(courseId) });
export const useActivity = (days: number) =>
  useQuery({ queryKey: sk.activity(days), queryFn: () => commands.getActivity(days) });
export const useWeeklyReview = (date: string | null) =>
  useQuery({ queryKey: sk.review(date), queryFn: () => commands.getWeeklyReview(date) });
export const usePlanned = (from: string, to: string) =>
  useQuery({ queryKey: sk.planned(from, to), queryFn: () => commands.getPlanned(from, to) });

export function useSaveStudySettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: StudySettings) => commands.setStudySettings(s),
    onSuccess: (s) => {
      qc.setQueryData(sk.settings, s);
      void qc.invalidateQueries({ queryKey: sk.all });
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}

export function useSetPlan(courseId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (target: Target | null) => commands.setCoursePlan(courseId, target),
    onSuccess: (p) => {
      qc.setQueryData(sk.plan(courseId), p);
      void qc.invalidateQueries({ queryKey: sk.all });
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}

export function useRebalance(courseId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => commands.rebalancePlan(courseId),
    onSuccess: (p) => {
      qc.setQueryData(sk.plan(courseId), p);
      void qc.invalidateQueries({ queryKey: sk.all });
      toast.success("Plan rebalanced from today", {
        description: p.order === "custom" ? "Kept the AI plan's order." : "In course outline order.",
      });
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}

/** Switches a course's plan between outline order and an applied AI plan's order. */
export function useSetPlanOrder(courseId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (order: "outline" | "custom") => commands.setPlanOrder(courseId, order),
    onSuccess: (p) => {
      qc.setQueryData(sk.plan(courseId), p);
      void qc.invalidateQueries({ queryKey: sk.all });
      toast.success("Plan now follows the course outline");
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
}

/** Root-level: refresh outlines and dashboards when watching auto-completes a lesson. */
export function useStudyEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const un = events.lessonCompleted.listen(() => {
      for (const k of [["course"], ["courses"], ["lesson"], sk.all])
        void qc.invalidateQueries({ queryKey: k });
    });
    return () => void un.then((f) => f());
  }, [qc]);
}

const TICK_SEC = 15;
const IDLE_SEC = 60;

/**
 * Counts note-taking time (SPEC §9): keystrokes in editors and inputs keep the user "active" for
 * 60 s; every 15 s of activity is recorded. Skipped while the video plays (that time already counts).
 */
export function useNotesActivity(lessonId: string | null) {
  useEffect(() => {
    if (!lessonId) return;
    let last = 0;
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el?.closest?.(".ProseMirror, textarea, input")) last = Date.now();
    };
    document.addEventListener("keydown", onKey, true);
    const timer = setInterval(() => {
      if (Date.now() - last > IDLE_SEC * 1000 || !document.hasFocus()) return;
      if (usePlayer.getState().playing) return;
      void commands.recordActivity(lessonId, "notes", TICK_SEC).catch(() => undefined);
    }, TICK_SEC * 1000);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      clearInterval(timer);
    };
  }, [lessonId]);
}
