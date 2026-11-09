/**
 * PlayerAdapter for downloaded files (Download mode / offline), backed by an HTML5 <video>.
 * Exposes extras the YouTube embed can't offer: frame stepping and picture-in-picture.
 */
import type { PlayerAdapter, PlayerState } from "./player-adapter";

export interface LocalAdapter extends PlayerAdapter {
  kind: "local";
  stepFrame(direction: 1 | -1): void;
  togglePictureInPicture(): Promise<void>;
  getVolume(): number;
}

export const FRAME_SEC = 1 / 30;

export function createLocalAdapter(
  host: HTMLElement,
  opts: {
    src: string;
    startSeconds?: number;
    rate?: number;
    onReady?: () => void;
    onError?: (message: string) => void;
  },
): LocalAdapter {
  const video = document.createElement("video");
  video.src = opts.src;
  video.preload = "auto";
  video.playsInline = true;
  video.style.cssText = "width:100%;height:100%;background:#000;object-fit:contain";
  host.replaceChildren(video);

  const timeCbs = new Set<(s: number) => void>();
  const stateCbs = new Set<(s: PlayerState) => void>();
  let state: PlayerState = "unstarted";
  const setState = (s: PlayerState) => {
    if (s === state) return;
    state = s;
    stateCbs.forEach((cb) => cb(s));
  };
  const emitTime = () => timeCbs.forEach((cb) => cb(video.currentTime));

  // timeupdate fires ~4×/s; a rAF loop while playing keeps the transcript highlight smooth.
  let raf = 0;
  const tick = () => {
    emitTime();
    raf = requestAnimationFrame(tick);
  };
  const on = (ev: string, fn: () => void) => video.addEventListener(ev, fn);
  on("loadedmetadata", () => {
    if (opts.startSeconds)
      video.currentTime = Math.min(opts.startSeconds, video.duration || opts.startSeconds);
    if (opts.rate) video.playbackRate = opts.rate;
    opts.onReady?.();
  });
  on("play", () => {
    setState("playing");
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(tick);
  });
  on("pause", () => {
    cancelAnimationFrame(raf);
    setState("paused");
    emitTime();
  });
  on("waiting", () => setState("buffering"));
  on("playing", () => setState("playing"));
  on("ended", () => {
    cancelAnimationFrame(raf);
    setState("ended");
  });
  on("seeked", emitTime);
  on("error", () =>
    opts.onError?.("This downloaded file can't be played. Try deleting and downloading it again."),
  );

  return {
    kind: "local",
    play: () => void video.play().catch(() => undefined),
    pause: () => video.pause(),
    seek: (s) => {
      video.currentTime = Math.max(0, Math.min(s, video.duration || s));
      emitTime();
    },
    getTime: () => video.currentTime,
    getDuration: () => (Number.isFinite(video.duration) ? video.duration : 0),
    isPlaying: () => !video.paused && !video.ended,
    setRate: (r) => (video.playbackRate = r),
    getRate: () => video.playbackRate,
    setVolume: (v) => {
      video.volume = Math.max(0, Math.min(1, v));
      video.muted = v === 0;
    },
    getVolume: () => (video.muted ? 0 : video.volume),
    stepFrame: (dir) => {
      video.pause();
      video.currentTime = Math.max(0, video.currentTime + dir * FRAME_SEC);
      emitTime();
    },
    togglePictureInPicture: async () => {
      if (document.pictureInPictureElement) await document.exitPictureInPicture();
      else if (document.pictureInPictureEnabled) await video.requestPictureInPicture();
    },
    onTimeUpdate: (cb) => {
      timeCbs.add(cb);
      return () => timeCbs.delete(cb);
    },
    onStateChange: (cb) => {
      stateCbs.add(cb);
      return () => stateCbs.delete(cb);
    },
    destroy: () => {
      cancelAnimationFrame(raf);
      if (document.pictureInPictureElement === video)
        void document.exitPictureInPicture().catch(() => undefined);
      video.pause();
      video.removeAttribute("src");
      video.load();
      timeCbs.clear();
      stateCbs.clear();
      video.remove();
    },
  };
}
