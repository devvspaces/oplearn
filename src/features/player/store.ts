/**
 * Shared playback state so side panels (annotations, transcript) can read the current time
 * and control the player without prop-drilling. StudyPlayer registers its controls here.
 */
import { create } from "zustand";

export interface PlayerControls {
  seek: (sec: number) => void;
  play: () => void;
  pause: () => void;
  isPlaying: () => boolean;
}

interface PlayerStore {
  lessonId: string | null;
  time: number;
  duration: number;
  playing: boolean;
  controls: PlayerControls | null;
  set: (p: Partial<Omit<PlayerStore, "set">>) => void;
}

export const usePlayer = create<PlayerStore>((set) => ({
  lessonId: null,
  time: 0,
  duration: 0,
  playing: false,
  controls: null,
  set: (p) => set(p),
}));

/** Imperative helpers for event handlers. */
export const player = {
  seek: (sec: number) => usePlayer.getState().controls?.seek(sec),
  pause: () => usePlayer.getState().controls?.pause(),
  play: () => usePlayer.getState().controls?.play(),
  time: () => usePlayer.getState().time,
};
