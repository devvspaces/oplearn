import { create } from "zustand";

/**
 * The PDF currently open in a viewer: its page (for note anchors and hotkeys) and a way to
 * scroll it, so panels can jump to "p.12" the way they seek the player.
 */
interface PdfNav {
  materialId: string | null;
  page: number;
  pages: number;
  goTo: ((page: number) => void) | null;
  set: (s: Partial<Omit<PdfNav, "set">>) => void;
}

export const usePdfNav = create<PdfNav>()((set) => ({
  materialId: null,
  page: 1,
  pages: 0,
  goTo: null,
  set: (s) => set(s),
}));

export const pdfNav = {
  current: () => {
    const s = usePdfNav.getState();
    return s.materialId ? { materialId: s.materialId, page: s.page } : null;
  },
  /** Scrolls the open viewer when it shows `materialId`; returns false otherwise. */
  goTo: (materialId: string, page: number) => {
    const s = usePdfNav.getState();
    if (s.materialId !== materialId || !s.goTo) return false;
    s.goTo(page);
    return true;
  },
};
