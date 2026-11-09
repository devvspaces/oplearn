import { useMemo, useRef, useState } from "react";
import { HoverCard } from "radix-ui";
import type { Annotation, Chapter } from "@/bindings";
import { formatTimestamp } from "@/lib/time";
import { groupMarkers, plainText, TYPE_META } from "@/features/annotations/timeline";
import { buildSegments, chapterAt, segmentFill, timeAtPointer } from "./segments";
import { cn } from "@/lib/utils";

/**
 * YouTube-style seek bar: chapter segments with gaps, a hover label ("12:34 · Chapter"),
 * and annotation dots above it with hover cards.
 */
export function SeekBar({
  time,
  duration,
  chapters,
  annotations,
  portalContainer,
  onSeek,
  onMarkerSelect,
}: {
  time: number;
  duration: number;
  chapters: Chapter[];
  annotations: Annotation[];
  /** Where hover cards render (the player frame), so they stay visible in fullscreen. */
  portalContainer?: HTMLElement | null;
  onSeek: (t: number) => void;
  onMarkerSelect: (a: Annotation) => void;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<{ x: number; t: number } | null>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const segments = useMemo(() => buildSegments(chapters, duration), [chapters, duration]);
  const groups = useMemo(() => groupMarkers(annotations, duration), [annotations, duration]);
  const shown = drag ?? time;
  const hasChapters = segments.some((s) => s.title);

  const pointer = (e: React.PointerEvent) => {
    const r = trackRef.current!.getBoundingClientRect();
    const x = Math.min(r.width, Math.max(0, e.clientX - r.left));
    return { x, t: timeAtPointer(x, r.width, duration) };
  };

  const hoverSeg = hover ? chapterAt(segments, hover.t) : null;
  const currentSeg = chapterAt(segments, shown);
  const label = (t: number) => {
    const c = chapterAt(segments, t)?.title;
    return c ? `${formatTimestamp(t)} · ${c}` : formatTimestamp(t);
  };

  return (
    <div className="select-none">
      {/* Annotation dots */}
      <div className="relative mb-1 h-3">
        {groups.map((g) => {
          const first = g.items[0]!;
          return (
            <HoverCard.Root key={first.id} openDelay={80} closeDelay={120}>
              <HoverCard.Trigger asChild>
                <button
                  aria-label={`${g.items.length > 1 ? `${g.items.length} items` : TYPE_META[first.type].label} at ${formatTimestamp(first.startSec ?? 0)}`}
                  onClick={() => {
                    onSeek(first.startSec ?? 0);
                    onMarkerSelect(first);
                  }}
                  className="absolute top-0 size-2.5 -translate-x-1/2 rounded-full ring-2 ring-black/60 transition-transform hover:scale-150 focus-visible:scale-150"
                  style={{ left: `${g.percent}%`, background: TYPE_META[first.type].color }}
                />
              </HoverCard.Trigger>
              <HoverCard.Portal container={portalContainer ?? undefined}>
                <HoverCard.Content
                  side="top"
                  sideOffset={8}
                  collisionPadding={8}
                  className="z-50 w-72 rounded-lg border bg-popover p-1.5 text-popover-foreground shadow-xl data-[state=open]:animate-in data-[state=open]:fade-in-0"
                >
                  <ul className="max-h-64 space-y-0.5 overflow-y-auto">
                    {g.items.map((a) => {
                      const text = a.title
                        ? `${a.title}${a.bodyMd ? ` — ${plainText(a.bodyMd, 100)}` : ""}`
                        : plainText(a.bodyMd);
                      return (
                        <li key={a.id}>
                          <button
                            onClick={() => {
                              onSeek(a.startSec ?? 0);
                              onMarkerSelect(a);
                            }}
                            className="flex w-full gap-2 rounded-md p-1.5 text-left hover:bg-accent"
                          >
                            <span
                              className="mt-1.5 size-2 shrink-0 rounded-full"
                              style={{ background: TYPE_META[a.type].color }}
                            />
                            <span className="min-w-0 flex-1">
                              <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                                <span className="font-medium text-foreground">{TYPE_META[a.type].label}</span>
                                <span className="font-mono text-primary">
                                  {formatTimestamp(a.startSec ?? 0)}
                                </span>
                                {(a.type === "question" || a.type === "todo") && a.status && (
                                  <span className="capitalize">· {a.status.replace("_", " ")}</span>
                                )}
                              </span>
                              <span className="line-clamp-2 text-xs">
                                {text || <em className="text-muted-foreground">No text</em>}
                              </span>
                            </span>
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                </HoverCard.Content>
              </HoverCard.Portal>
            </HoverCard.Root>
          );
        })}
      </div>

      {/* Track */}
      <div
        ref={trackRef}
        role="slider"
        tabIndex={0}
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(shown)}
        aria-valuetext={label(shown)}
        className="group/seek relative flex h-4 cursor-pointer items-center outline-none"
        onPointerMove={(e) => {
          const p = pointer(e);
          setHover(p);
          if (drag != null) setDrag(p.t);
        }}
        onPointerLeave={() => drag == null && setHover(null)}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          setDrag(pointer(e).t);
        }}
        onPointerUp={(e) => {
          if (drag == null) return;
          const t = pointer(e).t;
          setDrag(null);
          onSeek(t);
          if (e.pointerType !== "mouse") setHover(null);
        }}
        onKeyDown={(e) => {
          const step = e.shiftKey ? 30 : 5;
          if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
            e.preventDefault();
            e.stopPropagation();
            onSeek(Math.min(duration, Math.max(0, time + (e.key === "ArrowLeft" ? -step : step))));
          } else if (e.key === "Home") onSeek(0);
          else if (e.key === "End") onSeek(Math.max(0, duration - 1));
        }}
      >
        {segments.map((s, i) => {
          const fill = segmentFill(s, shown);
          const hovered = hoverSeg === s;
          return (
            <div
              key={i}
              className="absolute top-1/2 -translate-y-1/2"
              style={{
                left: `${s.leftPct}%`,
                width: `calc(${s.widthPct}% - ${hasChapters && i < segments.length - 1 ? 2 : 0}px)`,
              }}
            >
              <div
                className={cn(
                  "relative h-1 overflow-hidden rounded-full bg-white/25 transition-[height] group-focus-visible/seek:h-1.5",
                  hovered && "h-1.5",
                )}
              >
                {hovered && hover && (
                  <div
                    className="absolute inset-y-0 left-0 bg-white/30"
                    style={{ width: `${segmentFill(s, hover.t) * 100}%` }}
                  />
                )}
                <div
                  className="absolute inset-y-0 left-0 bg-[var(--primary)]"
                  style={{ width: `${fill * 100}%` }}
                />
              </div>
            </div>
          );
        })}
        {/* Playhead */}
        {duration > 0 && (
          <div
            className="pointer-events-none absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 scale-0 rounded-full bg-[var(--primary)] transition-transform group-hover/seek:scale-100 group-focus-visible/seek:scale-100"
            style={{ left: `${(shown / duration) * 100}%` }}
          />
        )}
        {/* Hover label */}
        {hover && duration > 0 && (
          <div
            className="pointer-events-none absolute bottom-5 -translate-x-1/2 whitespace-nowrap rounded bg-black/85 px-2 py-0.5 text-[11px] text-white shadow"
            style={{ left: `clamp(48px, ${hover.x}px, calc(100% - 48px))` }}
          >
            {label(hover.t)}
          </div>
        )}
      </div>
      {hasChapters && currentSeg?.title && <span className="sr-only">Chapter: {currentSeg.title}</span>}
    </div>
  );
}
