/**
 * Pending-write registry. When the app is about to quit, Rust emits `flush-request`;
 * we run every registered flusher (e.g. save playback position), then tell Rust to exit.
 */
import { listen } from "@tauri-apps/api/event";
import { commands } from "@/bindings";

const flushers = new Set<() => Promise<unknown> | void>();

export function registerFlusher(fn: () => Promise<unknown> | void): () => void {
  flushers.add(fn);
  return () => flushers.delete(fn);
}

export async function flushAll(): Promise<void> {
  await Promise.allSettled([...flushers].map((f) => f()));
}

let installed = false;
export function installFlushListener() {
  if (installed || !("__TAURI_INTERNALS__" in window)) return;
  installed = true;
  void listen("flush-request", async () => {
    await flushAll();
    await commands.flushComplete();
  });
}
