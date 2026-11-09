import { useCallback, useEffect, useRef } from "react";
import { registerFlusher } from "./flush";

/**
 * Debounced persistence: `schedule(v)` saves after `delay` ms of quiet; pending saves are
 * flushed on unmount and when the app quits (via the flush registry).
 */
export function useAutosave<T>(save: (v: T) => Promise<unknown>, delay = 600) {
  const saveRef = useRef(save);
  saveRef.current = save;
  const pending = useRef<{ v: T } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const flush = useCallback(() => {
    clearTimeout(timer.current);
    const p = pending.current;
    pending.current = null;
    return p ? saveRef.current(p.v) : undefined;
  }, []);

  const schedule = useCallback(
    (v: T) => {
      pending.current = { v };
      clearTimeout(timer.current);
      timer.current = setTimeout(() => void flush(), delay);
    },
    [delay, flush],
  );

  useEffect(() => {
    const unregister = registerFlusher(flush);
    return () => {
      unregister();
      void flush();
    };
  }, [flush]);

  return { schedule, flush };
}
