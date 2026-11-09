import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Theme = "dark" | "light" | "system";

interface UiState {
  theme: Theme;
  setTheme: (t: Theme) => void;
  libraryFilter: "all" | "in_progress" | "not_started" | "completed" | "archived";
  setLibraryFilter: (f: UiState["libraryFilter"]) => void;
  /** Lesson side panel. */
  panelTab: "notes" | "transcript" | "notebook" | "cards" | "quiz";
  setPanelTab: (t: UiState["panelTab"]) => void;
  panelWidth: number;
  setPanelWidth: (w: number) => void;
  /** Pause playback when creating a note/question/to-do (SPEC §7.4). */
  pauseOnNote: boolean;
  setPauseOnNote: (v: boolean) => void;
}

export const useUi = create<UiState>()(
  persist(
    (set) => ({
      theme: "dark",
      setTheme: (theme) => set({ theme }),
      libraryFilter: "all",
      setLibraryFilter: (libraryFilter) => set({ libraryFilter }),
      panelTab: "notes",
      setPanelTab: (panelTab) => set({ panelTab }),
      panelWidth: 380,
      setPanelWidth: (panelWidth) =>
        set({ panelWidth: Math.round(Math.min(720, Math.max(280, panelWidth))) }),
      pauseOnNote: true,
      setPauseOnNote: (pauseOnNote) => set({ pauseOnNote }),
    }),
    { name: "lectern-ui" },
  ),
);

export function applyTheme(theme: Theme) {
  const dark =
    theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
}
