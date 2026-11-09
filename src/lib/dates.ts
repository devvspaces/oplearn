/** Local-calendar date helpers working on `YYYY-MM-DD` strings (no time zones involved). */

export function ymd(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function parseYmd(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y!, (m ?? 1) - 1, d ?? 1);
}

export const todayYmd = () => ymd(new Date());

export function addDays(s: string, n: number): string {
  const d = parseYmd(s);
  d.setDate(d.getDate() + n);
  return ymd(d);
}

/** Monday of the week containing `s`. */
export function weekStart(s: string): string {
  const d = parseYmd(s);
  return addDays(s, -((d.getDay() + 6) % 7));
}

export function formatDay(
  s: string,
  opts: Intl.DateTimeFormatOptions = { weekday: "short", day: "numeric", month: "short" },
) {
  return parseYmd(s).toLocaleDateString(undefined, opts);
}

/** "Today", "Tomorrow", "Yesterday", or a short date. */
export function relativeDay(s: string, today = todayYmd()): string {
  if (s === today) return "Today";
  if (s === addDays(today, 1)) return "Tomorrow";
  if (s === addDays(today, -1)) return "Yesterday";
  return formatDay(s);
}

/** Heatmap level 0–4 for minutes relative to the daily goal. */
export function heatLevel(minutes: number, goal: number): 0 | 1 | 2 | 3 | 4 {
  if (minutes < 1) return 0;
  const r = minutes / Math.max(1, goal);
  if (r < 0.34) return 1;
  if (r < 0.67) return 2;
  if (r < 1) return 3;
  return 4;
}

/** Splits consecutive days (oldest first) into Monday-first week columns, padding the first. */
export function toWeeks<T extends { date: string }>(days: T[]): Array<Array<T | null>> {
  if (!days.length) return [];
  const pad = (parseYmd(days[0]!.date).getDay() + 6) % 7;
  const cells: Array<T | null> = [...Array<null>(pad).fill(null), ...days];
  const weeks: Array<Array<T | null>> = [];
  for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
  return weeks;
}

/** 6×7 Monday-first grid of `YYYY-MM-DD` dates covering the month of `s`. */
export function monthGrid(s: string): string[][] {
  const d = parseYmd(s);
  const first = ymd(new Date(d.getFullYear(), d.getMonth(), 1));
  const start = weekStart(first);
  return Array.from({ length: 6 }, (_, w) => Array.from({ length: 7 }, (_, i) => addDays(start, w * 7 + i)));
}

export function addMonths(s: string, n: number): string {
  const d = parseYmd(s);
  return ymd(new Date(d.getFullYear(), d.getMonth() + n, 1));
}

export const sameMonth = (a: string, b: string) => a.slice(0, 7) === b.slice(0, 7);
