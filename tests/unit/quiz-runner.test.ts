import { describe, expect, it } from "vitest";
import { isFinished, runReducer, score, shuffled, startRun } from "@/features/quiz/runner";

describe("quiz runner", () => {
  it("answers once, advances and scores", () => {
    let s = startRun(["a", "b", "c"]);
    s = runReducer(s, { type: "answer", id: "a", correct: true });
    s = runReducer(s, { type: "answer", id: "a", correct: false }); // ignored: already answered
    s = runReducer(s, { type: "next" });
    s = runReducer(s, { type: "answer", id: "b", correct: false });
    s = runReducer(s, { type: "next" });
    s = runReducer(s, { type: "answer", id: "c", correct: true });
    s = runReducer(s, { type: "next" });
    expect(isFinished(s)).toBe(true);
    expect(score(s)).toEqual({ correct: 2, answered: 3, total: 3 });
    const retry = runReducer(s, { type: "restart", ids: ["a", "b", "c"], wrongOnly: true });
    expect(retry.order).toEqual(["b"]);
    expect(retry.index).toBe(0);
  });

  it("shuffles without losing items", () => {
    const seq = [0.1, 0.9, 0.5];
    let i = 0;
    const r = shuffled([1, 2, 3, 4], () => seq[i++ % seq.length]!);
    expect(r.slice().sort()).toEqual([1, 2, 3, 4]);
  });
});
