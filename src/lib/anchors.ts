import { formatTimestamp } from "./time";

/** Where a note or card points: a video moment, or a page of a PDF material. */
export interface AnchorLike {
  startSec?: number | null;
  materialId?: string | null;
  page?: number | null;
}

/** "12:34" for a video moment, "p.3" for a PDF page, otherwise null. */
export function anchorLabel(a: AnchorLike): string | null {
  if (a.startSec != null) return formatTimestamp(a.startSec);
  if (a.page != null) return `p.${a.page}`;
  return null;
}

/** Search params for `/lessons/$lessonId` that open the lesson at the anchor. */
export function anchorSearch(a: AnchorLike): { t?: number; doc?: string; page?: number } {
  if (a.startSec != null) return { t: Math.floor(a.startSec) };
  if (a.page != null && a.materialId) return { doc: a.materialId, page: a.page };
  return {};
}

/** Orders by video time, then page; unanchored items last. */
export function compareAnchors(x: AnchorLike, y: AnchorLike): number {
  const t = (x.startSec ?? Infinity) - (y.startSec ?? Infinity);
  if (t !== 0 && Number.isFinite(t)) return t;
  if ((x.startSec == null) !== (y.startSec == null)) return x.startSec == null ? 1 : -1;
  return (x.page ?? Infinity) - (y.page ?? Infinity) || 0;
}

/** A flashcard's source as an anchor. */
export const cardAnchor = (c: {
  sourceSec?: number | null;
  sourceMaterialId?: string | null;
  sourcePage?: number | null;
}): AnchorLike => ({ startSec: c.sourceSec, materialId: c.sourceMaterialId, page: c.sourcePage });
