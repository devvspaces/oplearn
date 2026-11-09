import { describe, expect, it } from "vitest";
import { NS, isBridgeMessage, reduceBridge, type BridgeState } from "@/lib/youtube-iframe";

const init: BridgeState = { ready: false, state: "unstarted", time: 115, duration: 0, rate: 1 };

describe("YouTube bridge", () => {
  it("accepts only namespaced messages", () => {
    expect(isBridgeMessage({ ns: NS, type: "time", time: 1 })).toBe(true);
    expect(isBridgeMessage({ type: "time", time: 1 })).toBe(false);
    expect(isBridgeMessage("hello")).toBe(false);
    expect(isBridgeMessage(null)).toBe(false);
  });

  it("tracks ready, state, time and rate", () => {
    let s = reduceBridge(init, { ns: NS, type: "ready", duration: 4775, time: 115 });
    expect(s).toMatchObject({ ready: true, duration: 4775, time: 115 });
    s = reduceBridge(s, { ns: NS, type: "state", state: "playing", time: 116, duration: 0 });
    expect(s).toMatchObject({ state: "playing", time: 116, duration: 4775 });
    s = reduceBridge(s, { ns: NS, type: "time", time: 130.5 });
    expect(s.time).toBe(130.5);
    s = reduceBridge(s, { ns: NS, type: "rate", rate: 1.5 });
    expect(s.rate).toBe(1.5);
    expect(reduceBridge(s, { ns: NS, type: "error", code: 153 })).toBe(s);
  });
});
