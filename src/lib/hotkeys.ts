import { useEffect, useRef } from "react";

export type HotkeyMap = Record<string, (e: KeyboardEvent) => void>;

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target.isContentEditable;
}

/** Normalises an event into a key like `space`, `j`, `[`, `arrowleft`, `mod+k`. */
export function keyOf(e: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey">): string {
  const k = e.key === " " ? "space" : e.key.toLowerCase();
  return (e.metaKey || e.ctrlKey ? "mod+" : "") + (e.altKey ? "alt+" : "") + k;
}

/** Global single-key shortcuts, ignored while typing in a field. */
export function useHotkeys(map: HotkeyMap, enabled = true) {
  const ref = useRef(map);
  ref.current = map;
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || isTyping(e.target)) return;
      const fn = ref.current[keyOf(e)];
      if (fn) {
        e.preventDefault();
        fn(e);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled]);
}
