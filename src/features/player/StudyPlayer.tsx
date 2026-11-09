import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Camera,
  ChevronLeft,
  ChevronRight,
  HardDrive,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
  RotateCcw,
  RotateCw,
  Volume2,
  VolumeX,
} from "lucide-react";
import { createLocalAdapter, type LocalAdapter } from "@/lib/local-player";
import type { Annotation, Chapter } from "@/bindings";
import {
  clampRate,
  MAX_RATE,
  MIN_RATE,
  PositionSaver,
  RATE_STEP,
  resumePosition,
  type PlayerAdapter,
  type PlayerState,
} from "@/lib/player-adapter";
import { createYouTubeAdapter } from "@/lib/youtube-iframe";
import { registerFlusher } from "@/lib/flush";
import { useHotkeys } from "@/lib/hotkeys";
import { commands, errorMessage } from "@/lib/api";
import { formatTimestamp } from "@/lib/time";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/misc";
import { SeekBar } from "./SeekBar";
import { WatchTracker } from "./watch-tracker";
import { usePlayer } from "./store";
import { cn } from "@/lib/utils";

const RATES = Array.from(
  { length: (MAX_RATE - MIN_RATE) / RATE_STEP + 1 },
  (_, i) => MIN_RATE + i * RATE_STEP,
);
const ctrl = "text-white hover:bg-white/15 hover:text-white";

export function StudyPlayer({
  lessonId,
  videoId,
  lastPositionSec,
  durationSec,
  startAt,
  rate: initialRate,
  annotations,
  chapters,
  onRateChange,
  onEnded,
  onMarkerClick,
  localPath,
  onCaptureFrame,
  onAutoComplete,
}: {
  lessonId: string;
  videoId: string;
  lastPositionSec: number | null;
  durationSec: number | null;
  /** Explicit start (e.g. from search or an export link); overrides resume. */
  startAt?: number;
  /** Absolute path of a downloaded file; when set, plays locally (offline) instead of YouTube. */
  localPath?: string | null;
  /** Local playback only: capture the current frame into a note. */
  onCaptureFrame?: (sec: number) => void;
  rate: number;
  annotations: Annotation[];
  chapters: Chapter[];
  onRateChange: (rate: number) => void;
  onEnded?: () => void;
  onMarkerClick?: (a: Annotation) => void;
  /** Watching crossed the completion threshold (the lesson is now complete). */
  onAutoComplete?: () => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);
  const adapterRef = useRef<PlayerAdapter | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [state, setState] = useState<PlayerState>("unstarted");
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(durationSec ?? 0);
  const [rate, setRateState] = useState(initialRate);
  const [muted, setMuted] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const onEndedRef = useRef(onEnded);
  onEndedRef.current = onEnded;
  const onCompletedRef = useRef(onAutoComplete);
  onCompletedRef.current = onAutoComplete;
  const setStore = usePlayer((s) => s.set);

  // Mount one adapter per lesson. Resume = last position − 5 s, unless an explicit start was given.
  useEffect(() => {
    let cancelled = false;
    const saver = new PositionSaver((s) => commands.savePosition(lessonId, s));
    // Watched coverage + study minutes (SPEC §9), sent every 10 s and on pause/close.
    const tracker = new WatchTracker(() => adapterRef.current?.getRate() ?? 1);
    let isPlaying = false;
    const sendWatch = async () => {
      const b = tracker.take();
      if (!b) return;
      try {
        const r = await commands.recordWatch(lessonId, b.segments, b.wallSecs);
        if (r.completedNow) onCompletedRef.current?.();
      } catch {
        /* best effort: losing a few seconds of coverage is fine */
      }
    };
    const watchTimer = setInterval(() => void sendWatch(), 10_000);
    const unregister = registerFlusher(async () => {
      tracker.close();
      await Promise.all([saver.flush(), sendWatch()]);
    });
    const start = startAt ?? resumePosition(lastPositionSec, durationSec);
    setReady(false);
    setError(null);
    setTime(start);
    setStore({ lessonId, time: start, duration: durationSec ?? 0, playing: false });
    let unsubs: Array<() => void> = [];
    const onReady = () => {
      if (cancelled) return;
      setReady(true);
      const d = adapterRef.current?.getDuration();
      if (d) {
        setDuration(d);
        setStore({ duration: d });
      }
    };
    const adapterPromise: Promise<PlayerAdapter> = localPath
      ? // Served over loopback HTTP: WKWebView plays asset:// video without audio.
        commands.getMediaUrl(localPath).then((src) => {
          // Don't touch the host if the lesson changed while the URL resolved.
          if (cancelled) throw new Error("cancelled");
          return createLocalAdapter(hostRef.current!, {
            src,
            startSeconds: start,
            rate: initialRate,
            onReady,
            onError: (message) => !cancelled && setError(message),
          });
        })
      : createYouTubeAdapter(hostRef.current!, {
          videoId,
          startSeconds: start,
          rate: initialRate,
          onReady,
          onError: (_code, message) => !cancelled && setError(message),
        });
    adapterPromise
      .then((a) => {
        if (cancelled) return a.destroy();
        adapterRef.current = a;
        setStore({
          controls: {
            seek: (s) => a.seek(Math.max(0, s)),
            play: () => a.play(),
            pause: () => a.pause(),
            isPlaying: () => a.isPlaying(),
          },
        });
        unsubs = [
          a.onTimeUpdate((t) => {
            setTime(t);
            setStore({ time: t });
            saver.update(t);
            tracker.tick(
              t,
              performance.now(),
              isPlaying && (document.hasFocus() || !!document.pictureInPictureElement),
            );
          }),
          a.onStateChange((s) => {
            setState(s);
            isPlaying = s === "playing";
            if (s !== "playing") {
              tracker.close();
              if (s === "paused" || s === "ended") void sendWatch();
            }
            setStore({ playing: s === "playing" || s === "buffering" });
            if (s === "paused" || s === "ended") void saver.flush();
            if (s === "ended") onEndedRef.current?.();
            const d = a.getDuration();
            if (d) {
              setDuration(d);
              setStore({ duration: d });
            }
          }),
        ];
      })
      .catch((e: unknown) => !cancelled && setError(errorMessage(e)));

    const onHide = () => document.visibilityState === "hidden" && void saver.flush();
    document.addEventListener("visibilitychange", onHide);
    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", onHide);
      clearInterval(watchTimer);
      tracker.close();
      void sendWatch();
      const a = adapterRef.current;
      if (a) saver.update(a.getTime());
      void saver.flush();
      unregister();
      unsubs.forEach((u) => u());
      a?.destroy();
      adapterRef.current = null;
      setStore({ controls: null, playing: false });
    };
    // Rate and start position are initial values only; changing lessons remounts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lessonId, videoId, localPath]);

  // A new explicit start on the same lesson (e.g. a search result) seeks the running player.
  const firstStart = useRef(startAt);
  useEffect(() => {
    if (startAt == null || startAt === firstStart.current) return;
    firstStart.current = startAt;
    adapterRef.current?.seek(startAt);
  }, [startAt]);

  useEffect(() => {
    const fn = () => setFullscreen(document.fullscreenElement === frameRef.current);
    document.addEventListener("fullscreenchange", fn);
    return () => document.removeEventListener("fullscreenchange", fn);
  }, []);

  const a = () => adapterRef.current;
  const playing = state === "playing" || state === "buffering";
  const togglePlay = useCallback(() => {
    const p = a();
    if (!p) return;
    if (p.isPlaying()) p.pause();
    else p.play();
  }, []);
  const seekTo = (t: number) => {
    setTime(t);
    a()?.seek(t);
  };
  const seekBy = (d: number) => {
    const p = a();
    if (p) seekTo(Math.min(Math.max(0, p.getTime() + d), p.getDuration() || Infinity));
  };
  const changeRate = (r: number) => {
    const next = clampRate(r);
    a()?.setRate(next);
    setRateState(next);
    onRateChange(next);
  };
  const toggleFullscreen = () => {
    if (document.fullscreenElement) void document.exitFullscreen();
    else void frameRef.current?.requestFullscreen();
  };
  const [volume, setVolumeState] = useState(1);
  const toggleMute = () => {
    a()?.setVolume(muted ? volume || 1 : 0);
    setMuted(!muted);
  };
  const local = (): LocalAdapter | null => {
    const p = a();
    return p && "kind" in p && (p as LocalAdapter).kind === "local" ? (p as LocalAdapter) : null;
  };
  const stepFrame = (dir: 1 | -1) => local()?.stepFrame(dir);

  useHotkeys({
    space: togglePlay,
    k: togglePlay,
    j: () => seekBy(-10),
    l: () => seekBy(10),
    arrowleft: () => seekBy(-5),
    arrowright: () => seekBy(5),
    "[": () => changeRate(rate - RATE_STEP),
    "]": () => changeRate(rate + RATE_STEP),
    f: toggleFullscreen,
    m: toggleMute,
    ",": () => stepFrame(-1),
    ".": () => stepFrame(1),
  });

  const currentChapter = useMemo(
    () => [...chapters].reverse().find((c) => (c.startSec ?? 0) <= time)?.title,
    [chapters, time],
  );

  return (
    <div
      ref={frameRef}
      className={cn(
        "group/player relative overflow-hidden rounded-xl bg-black",
        fullscreen ? "h-full" : "aspect-video",
      )}
    >
      <div ref={hostRef} className="absolute inset-0 [&_iframe]:size-full" />
      {/* Overlay keeps focus (and our shortcuts) in the app instead of the cross-origin iframe. */}
      <button
        aria-label={playing ? "Pause" : "Play"}
        className="absolute inset-0 bottom-16 cursor-pointer"
        onClick={togglePlay}
        onDoubleClick={toggleFullscreen}
      />
      {!ready && !error && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-white/80">
          <Spinner className="size-6" />
        </div>
      )}
      {error && (
        <div
          role="alert"
          className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/80 p-6 text-center text-sm text-white"
        >
          <p>{error}</p>
          <a
            className="underline"
            href={`https://www.youtube.com/watch?v=${videoId}`}
            target="_blank"
            rel="noreferrer"
          >
            Watch on YouTube
          </a>
        </div>
      )}
      <div
        className={cn(
          "absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/90 to-transparent px-3 pb-2 pt-8 text-white transition-opacity",
          playing ? "opacity-0 group-hover/player:opacity-100 focus-within:opacity-100" : "opacity-100",
        )}
      >
        <SeekBar
          time={time}
          duration={duration}
          chapters={chapters}
          annotations={annotations}
          portalContainer={frameRef.current}
          onSeek={seekTo}
          onMarkerSelect={(m) => onMarkerClick?.(m)}
        />
        <div className="mt-1 flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            className={ctrl}
            aria-label={playing ? "Pause (Space)" : "Play (Space)"}
            onClick={togglePlay}
            disabled={!ready}
          >
            {playing ? <Pause /> : <Play />}
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className={ctrl}
            aria-label="Back 10 seconds (J)"
            onClick={() => seekBy(-10)}
          >
            <RotateCcw />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className={ctrl}
            aria-label="Forward 10 seconds (L)"
            onClick={() => seekBy(10)}
          >
            <RotateCw />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className={ctrl}
            aria-label={muted ? "Unmute (M)" : "Mute (M)"}
            onClick={toggleMute}
          >
            {muted ? <VolumeX /> : <Volume2 />}
          </Button>
          {localPath && (
            <input
              type="range"
              aria-label="Volume"
              min={0}
              max={1}
              step={0.05}
              value={muted ? 0 : volume}
              onChange={(e) => {
                const v = Number(e.target.value);
                setVolumeState(v);
                setMuted(v === 0);
                a()?.setVolume(v);
              }}
              className="h-1 w-16 cursor-pointer accent-white"
            />
          )}
          <span className="ml-1 shrink-0 text-xs tabular-nums text-white/90">
            {formatTimestamp(time)} / {formatTimestamp(duration)}
          </span>
          {currentChapter && (
            <span className="ml-2 min-w-0 truncate text-xs text-white/70">· {currentChapter}</span>
          )}
          <div className="flex-1" />
          {localPath && (
            <>
              <span
                className="mr-1 flex items-center gap-1 text-[10px] text-white/60"
                title="Playing the downloaded file"
              >
                <HardDrive className="size-3" /> Offline
              </span>
              <Button
                variant="ghost"
                size="icon-sm"
                className={ctrl}
                aria-label="Previous frame (,)"
                onClick={() => stepFrame(-1)}
              >
                <ChevronLeft />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                className={ctrl}
                aria-label="Next frame (.)"
                onClick={() => stepFrame(1)}
              >
                <ChevronRight />
              </Button>
              {onCaptureFrame && (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className={ctrl}
                  aria-label="Capture frame into a note"
                  onClick={() => {
                    a()?.pause();
                    onCaptureFrame(a()?.getTime() ?? time);
                  }}
                >
                  <Camera />
                </Button>
              )}
              <Button
                variant="ghost"
                size="icon-sm"
                className={ctrl}
                aria-label="Picture in picture"
                onClick={() => void local()?.togglePictureInPicture()}
              >
                <PictureInPicture2 />
              </Button>
            </>
          )}
          <label className="flex items-center text-xs">
            <span className="sr-only">Playback speed</span>
            <select
              value={rate}
              onChange={(e) => changeRate(Number(e.target.value))}
              className="rounded bg-white/10 px-1.5 py-0.5 text-xs text-white"
              title="Speed ([ / ])"
            >
              {RATES.map((r) => (
                <option key={r} value={r} className="text-black">
                  {r}×
                </option>
              ))}
            </select>
          </label>
          <Button
            variant="ghost"
            size="icon-sm"
            className={ctrl}
            aria-label="Fullscreen (F)"
            onClick={toggleFullscreen}
          >
            {fullscreen ? <Minimize /> : <Maximize />}
          </Button>
        </div>
      </div>
    </div>
  );
}
