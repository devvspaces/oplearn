import { useEffect } from "react";
import { Link, Outlet, createRootRouteWithContext, useNavigate, useRouterState } from "@tanstack/react-router";
import { useQuery, type QueryClient } from "@tanstack/react-query";
import { Toaster } from "sonner";
import {
  BookOpen,
  CalendarDays,
  Download,
  Home,
  Layers,
  Library,
  ListChecks,
  Search,
  Settings,
  MonitorPlay,
} from "lucide-react";
import { applyTheme, useUi } from "@/stores/ui";
import { installFlushListener } from "@/lib/flush";
import { setDataDir } from "@/lib/attachments";
import { commands } from "@/lib/api";
import { useDownloadEvents, useActiveDownloads } from "@/features/downloads/useDownloads";
import { CommandPalette } from "@/features/search/CommandPalette";
import { useStudyEvents, useToday } from "@/features/study/api";
import { CardDialog } from "@/features/flashcards/CardDialog";
import { cn } from "@/lib/utils";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  component: RootLayout,
});

const NAV = [
  { to: "/", label: "Today", icon: Home },
  { to: "/library", label: "Library", icon: Library },
  { to: "/planner", label: "Planner", icon: CalendarDays },
  { to: "/review", label: "Review", icon: Layers },
  { to: "/notes/$kind", label: "Questions & to-dos", icon: ListChecks },
  { to: "/search", label: "Discover", icon: MonitorPlay },
  { to: "/downloads", label: "Downloads", icon: Download },
  { to: "/settings", label: "Settings", icon: Settings },
] as const;

function RootLayout() {
  const theme = useUi((s) => s.theme);
  useEffect(() => installFlushListener(), []);
  useEffect(() => {
    void commands.getAppInfo().then(
      (i) => setDataDir(i.dataDir),
      () => undefined,
    );
  }, []);
  useDownloadEvents();
  useStudyEvents();
  const navigate = useNavigate();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const onboarded = useQuery({ queryKey: ["onboarded"], queryFn: commands.getOnboarded, staleTime: Infinity });
  useEffect(() => {
    if (onboarded.data === false && path !== "/welcome") void navigate({ to: "/welcome", replace: true });
  }, [onboarded.data, path, navigate]);
  const activeDownloads = useActiveDownloads();
  const dueCards = useToday().data?.dueCards ?? 0;
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const fn = () => applyTheme("system");
    mq.addEventListener("change", fn);
    return () => mq.removeEventListener("change", fn);
  }, [theme]);

  if (path === "/welcome")
    return (
      <div className="h-full overflow-y-auto">
        <Outlet />
        <Toaster theme={theme === "light" ? "light" : "dark"} position="bottom-right" richColors closeButton />
      </div>
    );

  return (
    <div className="flex h-full">
      <nav aria-label="Main" className="flex w-52 shrink-0 flex-col gap-1 border-r bg-card/40 p-3">
        <div className="mb-4 flex items-center gap-2 px-2 pt-1 text-base font-semibold">
          <BookOpen className="size-5 text-primary" aria-hidden />
          Lectern
        </div>
        <button
          onClick={() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }))}
          className="mb-2 flex items-center gap-2 rounded-md border bg-background/60 px-2.5 py-1.5 text-xs text-muted-foreground hover:text-foreground"
        >
          <Search className="size-3.5" aria-hidden /> Search
          <kbd className="ml-auto font-mono text-[10px]">⌘K</kbd>
        </button>
        {NAV.map(({ to, label, icon: Icon }) => (
          <Link
            key={to}
            to={to}
            params={to === "/notes/$kind" ? { kind: "questions" } : undefined}
            activeOptions={{ exact: to === "/", includeSearch: false }}
            className={cn(
              "flex items-center gap-2.5 rounded-md px-2.5 py-2 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
              "data-[status=active]:bg-accent data-[status=active]:text-foreground",
            )}
          >
            <Icon className="size-4" aria-hidden />
            {label}
            {to === "/review" && dueCards > 0 && (
              <span
                className="ml-auto rounded-full bg-muted px-1.5 text-[10px] font-semibold text-foreground"
                aria-label={`${dueCards} cards due`}
              >
                {dueCards}
              </span>
            )}
            {to === "/downloads" && activeDownloads > 0 && (
              <span
                className="ml-auto rounded-full bg-primary px-1.5 text-[10px] font-semibold text-primary-foreground"
                aria-label={`${activeDownloads} active downloads`}
              >
                {activeDownloads}
              </span>
            )}
          </Link>
        ))}
      </nav>
      <main className="min-w-0 flex-1 overflow-y-auto">
        <Outlet />
      </main>
      <CommandPalette />
      <CardDialog />
      <Toaster theme={theme === "light" ? "light" : "dark"} position="bottom-right" richColors closeButton />
    </div>
  );
}
