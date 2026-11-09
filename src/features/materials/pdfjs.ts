/** Lazy pdf.js loader: the library, its worker and the viewer components load on first use. */
type PdfJs = typeof import("pdfjs-dist");
type ViewerLib = typeof import("pdfjs-dist/web/pdf_viewer.mjs");

let loading: Promise<{ pdfjs: PdfJs; viewer: ViewerLib }> | null = null;

export function loadPdfjs() {
  loading ??= (async () => {
    const pdfjs = await import("pdfjs-dist");
    const worker = await import("pdfjs-dist/build/pdf.worker.min.mjs?url");
    pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
    // The viewer bundle reads the core library from this global when it loads.
    (globalThis as unknown as { pdfjsLib: PdfJs }).pdfjsLib = pdfjs;
    const viewer = await import("pdfjs-dist/web/pdf_viewer.mjs");
    await import("pdfjs-dist/web/pdf_viewer.css");
    return { pdfjs, viewer };
  })();
  loading.catch(() => {
    loading = null;
  });
  return loading;
}

/** Fonts and CMaps, copied into `public/pdfjs/` by vite.config.ts. */
export const PDF_ASSETS = {
  standardFontDataUrl: "/pdfjs/standard_fonts/",
  cMapUrl: "/pdfjs/cmaps/",
  cMapPacked: true,
};
