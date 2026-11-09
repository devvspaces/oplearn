import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronDown, ChevronUp, HelpCircle, Layers, Minus, MoveHorizontal, Plus, StickyNote } from "lucide-react";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/misc";
import { PDF_ASSETS, loadPdfjs } from "./pdfjs";
import { usePdfNav } from "./pdf-nav";

export type SelectionAction = "note" | "question" | "card";

type Viewer = InstanceType<typeof import("pdfjs-dist/web/pdf_viewer.mjs").PDFViewer>;
type Bus = InstanceType<typeof import("pdfjs-dist/web/pdf_viewer.mjs").EventBus>;

/**
 * Lectern's PDF viewer (pdf.js): continuous pages with a text layer, page tracking for note
 * anchors, find highlighting for ⌘K hits, and a popover to turn selected text into notes or cards.
 */
export function PdfViewer({
  url,
  materialId,
  page,
  query,
  onSelectText,
  toolbarEnd,
}: {
  url: string;
  materialId: string;
  page?: number | null;
  query?: string | null;
  onSelectText?: (action: SelectionAction, text: string, page: number) => void;
  toolbarEnd?: React.ReactNode;
}) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);
  const viewerRef = useRef<Viewer | null>(null);
  const busRef = useRef<Bus | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState("");
  const [current, setCurrent] = useState(page ?? 1);
  const [pages, setPages] = useState(0);
  const [pageInput, setPageInput] = useState(String(page ?? 1));
  const [sel, setSel] = useState<{ text: string; page: number; x: number; y: number } | null>(null);
  const setNav = usePdfNav((s) => s.set);
  const initial = useRef({ page, query });
  initial.current = { page, query };

  // Load the document whenever the file changes.
  useEffect(() => {
    let cancelled = false;
    let destroy: (() => void) | undefined;
    setState("loading");
    void (async () => {
      try {
        const { pdfjs, viewer: lib } = await loadPdfjs();
        if (cancelled || !containerRef.current || !innerRef.current) return;
        const eventBus = new lib.EventBus();
        const linkService = new lib.PDFLinkService({ eventBus });
        const findController = new lib.PDFFindController({ eventBus, linkService });
        const viewer = new lib.PDFViewer({
          container: containerRef.current,
          viewer: innerRef.current,
          eventBus,
          linkService,
          findController,
          textLayerMode: 1,
        });
        linkService.setViewer(viewer);
        const task = pdfjs.getDocument({ url, isEvalSupported: false, ...PDF_ASSETS });
        destroy = () => {
          void task.destroy();
          viewer.setDocument(null as never);
        };
        const doc = await task.promise;
        if (cancelled) return;
        eventBus.on("pagesinit", () => {
          viewer.currentScaleValue = "page-width";
          const { page: p, query: q } = initial.current;
          if (p && p > 1) viewer.currentPageNumber = Math.min(p, doc.numPages);
          if (q) find(eventBus, q);
        });
        eventBus.on("pagechanging", ({ pageNumber }: { pageNumber: number }) => {
          setCurrent(pageNumber);
          setPageInput(String(pageNumber));
          setNav({ page: pageNumber });
        });
        viewer.setDocument(doc);
        linkService.setDocument(doc);
        viewerRef.current = viewer;
        busRef.current = eventBus;
        setPages(doc.numPages);
        setCurrent(initial.current.page ?? 1);
        setNav({
          materialId,
          page: initial.current.page ?? 1,
          pages: doc.numPages,
          goTo: (n) => {
            viewer.currentPageNumber = Math.max(1, Math.min(doc.numPages, n));
          },
        });
        setState("ready");
        void backfillText(materialId, doc);
      } catch (e) {
        if (cancelled) return;
        setError(errorMessage(e));
        setState("error");
      }
    })();
    return () => {
      cancelled = true;
      destroy?.();
      viewerRef.current = null;
      busRef.current = null;
      if (usePdfNav.getState().materialId === materialId)
        setNav({ materialId: null, goTo: null, pages: 0, page: 1 });
    };
  }, [url, materialId, setNav]);

  // Later page / query changes (e.g. clicking "p.12" on a note, or a new ⌘K hit).
  useEffect(() => {
    const v = viewerRef.current;
    if (v && page && page !== v.currentPageNumber) v.currentPageNumber = page;
  }, [page]);
  useEffect(() => {
    if (busRef.current && query) find(busRef.current, query);
  }, [query]);

  const go = (n: number) => {
    const v = viewerRef.current;
    if (v && pages) v.currentPageNumber = Math.max(1, Math.min(pages, n));
  };
  const zoom = (f: number | "page-width") => {
    const v = viewerRef.current;
    if (!v) return;
    if (f === "page-width") v.currentScaleValue = "page-width";
    else v.currentScale = Math.max(0.3, Math.min(4, v.currentScale * f));
  };

  function onMouseUp() {
    if (!onSelectText) return;
    const s = window.getSelection();
    const text = s?.toString().trim() ?? "";
    if (!s || !text || s.rangeCount === 0 || !containerRef.current?.contains(s.anchorNode)) {
      setSel(null);
      return;
    }
    const pageEl = (s.anchorNode instanceof Element ? s.anchorNode : s.anchorNode?.parentElement)?.closest(
      ".page",
    );
    const p = Number(pageEl?.getAttribute("data-page-number")) || current;
    const r = s.getRangeAt(0).getBoundingClientRect();
    const box = wrapRef.current!.getBoundingClientRect();
    setSel({ text: text.slice(0, 2000), page: p, x: r.left - box.left + r.width / 2, y: r.top - box.top });
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-1 border-b px-2 py-1.5 text-xs">
        <Button size="icon-sm" variant="ghost" aria-label="Previous page" onClick={() => go(current - 1)}>
          <ChevronUp />
        </Button>
        <Button size="icon-sm" variant="ghost" aria-label="Next page" onClick={() => go(current + 1)}>
          <ChevronDown />
        </Button>
        <form
          className="flex items-center gap-1"
          onSubmit={(e) => {
            e.preventDefault();
            go(Number(pageInput) || 1);
          }}
        >
          <label htmlFor={`pdf-page-${materialId}`} className="text-muted-foreground">
            Page
          </label>
          <Input
            id={`pdf-page-${materialId}`}
            value={pageInput}
            onChange={(e) => setPageInput(e.target.value)}
            className="h-7 w-12 text-xs"
          />
          <span className="text-muted-foreground" aria-live="polite">
            of {pages || "…"}
          </span>
        </form>
        <span className="mx-1 h-4 w-px bg-border" aria-hidden />
        <Button size="icon-sm" variant="ghost" aria-label="Zoom out" onClick={() => zoom(1 / 1.15)}>
          <Minus />
        </Button>
        <Button size="icon-sm" variant="ghost" aria-label="Zoom in" onClick={() => zoom(1.15)}>
          <Plus />
        </Button>
        <Button size="icon-sm" variant="ghost" aria-label="Fit width" onClick={() => zoom("page-width")}>
          <MoveHorizontal />
        </Button>
        <div className="flex-1" />
        {toolbarEnd}
      </div>
      <div ref={wrapRef} className="relative min-h-0 flex-1 bg-muted/60">
        <div
          ref={containerRef}
          tabIndex={0}
          aria-label="PDF document"
          className="absolute inset-0 overflow-auto outline-none"
          onMouseUp={onMouseUp}
          onScroll={() => sel && setSel(null)}
          onClick={(e) => {
            // External links in the PDF open in the browser, never inside the app's webview.
            const a = (e.target as HTMLElement).closest("a");
            const href = a?.getAttribute("href") ?? "";
            if (/^https?:/i.test(href)) {
              e.preventDefault();
              void openUrl(href);
            }
          }}
        >
          <div ref={innerRef} className="pdfViewer" />
        </div>
        {state === "loading" && (
          <div className="absolute inset-0 grid place-items-center">
            <Spinner />
          </div>
        )}
        {state === "error" && (
          <div className="absolute inset-0 grid place-items-center p-6 text-center text-sm text-destructive">
            Couldn't open this PDF: {error}
          </div>
        )}
        {sel && onSelectText && (
          <div
            role="toolbar"
            aria-label="Selected text"
            className="absolute z-20 flex -translate-x-1/2 -translate-y-full gap-1 rounded-lg border bg-popover p-1 shadow-md"
            style={{ left: Math.max(80, sel.x), top: Math.max(40, sel.y - 6) }}
            onMouseDown={(e) => e.preventDefault()}
          >
            {(
              [
                ["note", "Note", StickyNote],
                ["question", "Question", HelpCircle],
                ["card", "Flashcard", Layers],
              ] as const
            ).map(([k, label, Icon]) => (
              <Button
                key={k}
                size="sm"
                variant="ghost"
                onClick={() => {
                  onSelectText(k, sel.text, sel.page);
                  setSel(null);
                  window.getSelection()?.removeAllRanges();
                }}
              >
                <Icon /> {label}
              </Button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function find(bus: Bus, query: string) {
  bus.dispatch("find", {
    type: "",
    query,
    caseSensitive: false,
    entireWord: false,
    highlightAll: true,
    findPrevious: false,
    matchDiacritics: false,
  });
}

/** If the import couldn't read this PDF's text, read it here so search and AI can use it. */
async function backfillText(
  materialId: string,
  doc: { numPages: number; getPage: (n: number) => Promise<{ getTextContent: () => Promise<{ items: unknown[] }> }> },
) {
  try {
    if ((await commands.getMaterialIndexedPages(materialId)) > 0 || doc.numPages > 3000) return;
    const pages: string[] = [];
    for (let i = 1; i <= doc.numPages; i++) {
      const tc = await (await doc.getPage(i)).getTextContent();
      pages.push(
        tc.items
          .map((it) => (it && typeof it === "object" && "str" in it ? String((it as { str: string }).str) : ""))
          .join(" "),
      );
    }
    if (pages.some((p) => p.trim())) await commands.setMaterialPages(materialId, pages);
  } catch {
    // Best effort: the PDF still shows; it just stays unsearchable.
  }
}
