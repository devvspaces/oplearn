/** Quiz session state (pure; unit-tested). */
export interface RunState {
  order: string[];
  index: number;
  results: Record<string, boolean>;
}

export type RunAction =
  | { type: "answer"; id: string; correct: boolean }
  | { type: "next" }
  | { type: "restart"; ids: string[]; wrongOnly?: boolean; shuffle?: (ids: string[]) => string[] };

export function startRun(ids: string[], shuffle: (ids: string[]) => string[] = (x) => x): RunState {
  return { order: shuffle(ids), index: 0, results: {} };
}

export function runReducer(s: RunState, a: RunAction): RunState {
  switch (a.type) {
    case "answer":
      if (a.id in s.results) return s;
      return { ...s, results: { ...s.results, [a.id]: a.correct } };
    case "next":
      return { ...s, index: Math.min(s.order.length, s.index + 1) };
    case "restart": {
      const ids = a.wrongOnly ? s.order.filter((id) => s.results[id] === false) : a.ids;
      return startRun(ids.length ? ids : a.ids, a.shuffle);
    }
  }
}

export const isFinished = (s: RunState) => s.index >= s.order.length;

export const score = (s: RunState) => {
  const vals = Object.values(s.results);
  return { correct: vals.filter(Boolean).length, answered: vals.length, total: s.order.length };
};

/** Fisher–Yates shuffle (copy). */
export function shuffled<T>(xs: T[], rand: () => number = Math.random): T[] {
  const a = xs.slice();
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    const tmp = a[i]!;
    a[i] = a[j]!;
    a[j] = tmp;
  }
  return a;
}
