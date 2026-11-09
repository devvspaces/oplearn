/**
 * PlayerAdapter for Stream mode. The official YouTube IFrame API runs inside a bridge page
 * served by the Rust core from http://127.0.0.1:<port>/player (src-tauri/assets/player.html).
 * That gives YouTube a valid http referrer; a packaged app's own origin (tauri://localhost)
 * gets "error 153". We talk to the bridge with postMessage.
 */
import { commands } from "@/bindings";
import type { PlayerAdapter, PlayerState } from "./player-adapter";

export const NS = "lectern-player";

export const YT_ERRORS: Record<number, string> = {
  [-1]: "Could not load the YouTube player. Check your internet connection.",
  2: "Invalid video id.",
  5: "This video can't be played in the HTML5 player.",
  100: "This video was removed or made private.",
  101: "The owner doesn't allow this video to be embedded.",
  150: "The owner doesn't allow this video to be embedded.",
  152: "YouTube refused the embed (player configuration error).",
  153: "YouTube refused the embed (missing referrer).",
};

export type BridgeMessage =
  | { ns: typeof NS; type: "ready"; duration: number; time: number }
  | { ns: typeof NS; type: "state"; state: PlayerState; time: number; duration: number }
  | { ns: typeof NS; type: "time"; time: number }
  | { ns: typeof NS; type: "rate"; rate: number }
  | { ns: typeof NS; type: "error"; code: number };

export interface BridgeState {
  ready: boolean;
  state: PlayerState;
  time: number;
  duration: number;
  rate: number;
}

export function isBridgeMessage(d: unknown): d is BridgeMessage {
  return (
    typeof d === "object" &&
    d !== null &&
    (d as { ns?: unknown }).ns === NS &&
    typeof (d as { type?: unknown }).type === "string"
  );
}

/** Pure reducer over bridge messages (unit-tested). */
export function reduceBridge(s: BridgeState, m: BridgeMessage): BridgeState {
  switch (m.type) {
    case "ready":
      return { ...s, ready: true, duration: m.duration || s.duration, time: m.time };
    case "state":
      return { ...s, state: m.state, time: m.time, duration: m.duration || s.duration };
    case "time":
      return { ...s, time: m.time };
    case "rate":
      return { ...s, rate: m.rate };
    case "error":
      return s;
  }
}

let playerUrl: Promise<string> | null = null;
function getPlayerUrl(): Promise<string> {
  playerUrl ??= commands.getPlayerUrl().catch((e: unknown) => {
    playerUrl = null;
    throw e;
  });
  return playerUrl;
}

export interface YouTubeAdapterOptions {
  videoId: string;
  startSeconds?: number;
  rate?: number;
  onReady?: () => void;
  onError?: (code: number, message: string) => void;
}

export async function createYouTubeAdapter(
  host: HTMLElement,
  opts: YouTubeAdapterOptions,
): Promise<PlayerAdapter> {
  const base = await getPlayerUrl();
  const origin = new URL(base).origin;
  const timeCbs = new Set<(s: number) => void>();
  const stateCbs = new Set<(s: PlayerState) => void>();
  let st: BridgeState = {
    ready: false,
    state: "unstarted",
    time: opts.startSeconds ?? 0,
    duration: 0,
    rate: opts.rate ?? 1,
  };

  const iframe = document.createElement("iframe");
  const q = new URLSearchParams({
    v: opts.videoId,
    start: String(Math.floor(opts.startSeconds ?? 0)),
    rate: String(opts.rate ?? 1),
  });
  iframe.src = `${base}?${q}`;
  iframe.title = "YouTube video player";
  iframe.allow = "autoplay; encrypted-media; picture-in-picture; fullscreen";
  iframe.referrerPolicy = "strict-origin-when-cross-origin";
  iframe.style.cssText = "width:100%;height:100%;border:0";
  host.replaceChildren(iframe);

  const send = (cmd: string, value?: number) =>
    iframe.contentWindow?.postMessage({ ns: NS, cmd, value }, origin);

  const onMessage = (e: MessageEvent) => {
    if (e.origin !== origin || e.source !== iframe.contentWindow || !isBridgeMessage(e.data)) return;
    const m = e.data;
    const prev = st;
    st = reduceBridge(st, m);
    if (m.type === "ready") opts.onReady?.();
    if (m.type === "error") opts.onError?.(m.code, YT_ERRORS[m.code] ?? `YouTube player error ${m.code}`);
    if (m.type === "state" && m.state !== prev.state) stateCbs.forEach((cb) => cb(m.state));
    if (m.type === "time" || m.type === "state") timeCbs.forEach((cb) => cb(st.time));
  };
  window.addEventListener("message", onMessage);

  return {
    play: () => send("play"),
    pause: () => send("pause"),
    seek: (s) => {
      st = { ...st, time: Math.max(0, s) };
      send("seek", Math.max(0, s));
    },
    getTime: () => st.time,
    getDuration: () => st.duration,
    isPlaying: () => st.state === "playing" || st.state === "buffering",
    setRate: (r) => {
      st = { ...st, rate: r };
      send("rate", r);
    },
    getRate: () => st.rate,
    setVolume: (v) => send("volume", v),
    onTimeUpdate: (cb) => {
      timeCbs.add(cb);
      return () => timeCbs.delete(cb);
    },
    onStateChange: (cb) => {
      stateCbs.add(cb);
      return () => stateCbs.delete(cb);
    },
    destroy: () => {
      window.removeEventListener("message", onMessage);
      timeCbs.clear();
      stateCbs.clear();
      iframe.remove();
    },
  };
}
