import { useState } from "react";
import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { BookOpen, Check, FolderOpen, GraduationCap, KeyRound, MonitorPlay, RotateCw } from "lucide-react";
import { commands, errorMessage, qk, useAppInfo } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input, Label } from "@/components/ui/input";
import { ImportPlaylistDialog } from "@/features/library/ImportPlaylistDialog";
import { OcwDiscover } from "@/features/ocw/OcwDiscover";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/welcome")({ component: Welcome });

const STEPS = ["Data folder", "Keys (optional)", "First course"] as const;

/** First-run flow: data folder → optional keys → a first course. */
function Welcome() {
  const [step, setStep] = useState(0);
  const navigate = useNavigate();
  const qc = useQueryClient();

  const finish = async (to: "/" | "/library" = "/") => {
    await commands.setOnboarded(true);
    qc.setQueryData(["onboarded"], true);
    void navigate({ to });
  };

  return (
    <div className="mx-auto flex min-h-full max-w-3xl flex-col gap-6 p-8">
      <header className="flex items-center gap-3">
        <BookOpen className="size-7 text-primary" aria-hidden />
        <div>
          <h1 className="text-2xl font-semibold">Welcome to Lectern</h1>
          <p className="text-sm text-muted-foreground">
            Turn open courses into a study plan, with notes, flashcards and quizzes, all on your computer.
          </p>
        </div>
      </header>
      <ol className="flex gap-2 text-xs" aria-label="Setup steps">
        {STEPS.map((s, i) => (
          <li
            key={s}
            aria-current={i === step ? "step" : undefined}
            className={cn(
              "flex items-center gap-1.5 rounded-full border px-3 py-1",
              i === step && "border-primary text-primary",
              i < step && "text-muted-foreground",
            )}
          >
            {i < step ? <Check className="size-3" /> : <span className="tabular-nums">{i + 1}</span>} {s}
          </li>
        ))}
      </ol>
      <section className="rounded-xl border bg-card p-6">
        {step === 0 && <DataStep onNext={() => setStep(1)} />}
        {step === 1 && (
          <KeysStep
            onNext={async () => {
              // Done from here on: importing a course navigates away from this page.
              await commands.setOnboarded(true);
              qc.setQueryData(["onboarded"], true);
              setStep(2);
            }}
          />
        )}
        {step === 2 && <CourseStep onSkip={() => void finish("/")} />}
      </section>
      {step > 0 && (
        <button
          className="self-start text-xs text-muted-foreground hover:text-foreground"
          onClick={() => setStep(step - 1)}
        >
          ← Back
        </button>
      )}
    </div>
  );
}

function DataStep({ onNext }: { onNext: () => void }) {
  const { data: info } = useAppInfo();
  const [changed, setChanged] = useState<string | null>(null);
  async function choose() {
    const dir = await openDialog({ directory: true, title: "Choose where Lectern keeps your library" });
    if (typeof dir !== "string") return;
    try {
      await commands.setDataDir(dir);
      setChanged(dir);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }
  return (
    <div className="space-y-4 text-sm">
      <h2 className="flex items-center gap-2 text-base font-semibold">
        <FolderOpen className="size-5 text-primary" /> Where should your library live?
      </h2>
      <p className="text-muted-foreground">
        Your courses, notes and progress are stored in one folder, along with any videos and PDFs you
        download. Pick an external drive if you plan to download a lot.
      </p>
      <code className="block truncate rounded bg-muted px-3 py-2 text-xs">{changed ?? info?.dataDir}</code>
      {changed ? (
        <div className="flex flex-wrap items-center gap-2">
          <p className="text-xs">Lectern will use this folder after it restarts.</p>
          <Button onClick={() => void commands.restartApp()}>
            <RotateCw /> Restart now
          </Button>
        </div>
      ) : (
        <div className="flex gap-2">
          <Button onClick={onNext}>Use this folder</Button>
          <Button variant="outline" onClick={() => void choose()}>
            Choose another…
          </Button>
        </div>
      )}
    </div>
  );
}

function KeysStep({ onNext }: { onNext: () => Promise<void> }) {
  const qc = useQueryClient();
  const [yt, setYt] = useState("");
  const [ai, setAi] = useState("");
  const [busy, setBusy] = useState(false);
  async function next() {
    setBusy(true);
    try {
      if (yt.trim()) await commands.setApiKey(yt.trim());
      if (ai.trim()) {
        await commands.setAiKey(ai.trim());
        await commands.setAiSettings({ enabled: true });
      }
      void qc.invalidateQueries({ queryKey: qk.appInfo });
      void qc.invalidateQueries({ queryKey: ["aiStatus"] });
      await onNext();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="space-y-5 text-sm">
      <h2 className="flex items-center gap-2 text-base font-semibold">
        <KeyRound className="size-5 text-primary" /> Optional keys
      </h2>
      <p className="text-muted-foreground">
        Lectern works without any keys. Each one unlocks more, and is stored in your system keychain. You can
        add them later in Settings.
      </p>
      <div className="grid gap-1">
        <Label htmlFor="w-yt">YouTube Data API key</Label>
        <Input
          id="w-yt"
          type="password"
          value={yt}
          onChange={(e) => setYt(e.target.value)}
          placeholder="AIza…"
        />
        <p className="text-xs text-muted-foreground">
          Faster playlist imports, Creative Commons license badges and YouTube search filters.
        </p>
      </div>
      <div className="grid gap-1">
        <Label htmlFor="w-ai">Anthropic API key</Label>
        <Input
          id="w-ai"
          type="password"
          value={ai}
          onChange={(e) => setAi(e.target.value)}
          placeholder="sk-ant-…"
        />
        <p className="text-xs text-muted-foreground">
          AI flashcards, quizzes, study plans and explanations. Nothing is sent until you click an AI button,
          and every request shows its cost first.
        </p>
      </div>
      <Button onClick={() => void next()} disabled={busy}>
        {yt.trim() || ai.trim() ? "Save and continue" : "Skip for now"}
      </Button>
    </div>
  );
}

function CourseStep({ onSkip }: { onSkip: () => void }) {
  const [source, setSource] = useState<"youtube" | "ocw" | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  return (
    <div className="space-y-4 text-sm">
      <h2 className="text-base font-semibold">Add your first course</h2>
      <div className="grid gap-3 sm:grid-cols-2">
        <button
          className="rounded-xl border p-4 text-left hover:border-primary/60"
          onClick={() => {
            setSource("youtube");
            setImportOpen(true);
          }}
        >
          <MonitorPlay className="mb-2 size-6 text-primary" />
          <p className="font-medium">From a YouTube playlist</p>
          <p className="mt-1 text-xs text-muted-foreground">
            Paste a playlist link; each video becomes a lesson.
          </p>
        </button>
        <button
          className={cn(
            "rounded-xl border p-4 text-left hover:border-primary/60",
            source === "ocw" && "border-primary",
          )}
          onClick={() => setSource("ocw")}
        >
          <GraduationCap className="mb-2 size-6 text-primary" />
          <p className="font-medium">From MIT OpenCourseWare</p>
          <p className="mt-1 text-xs text-muted-foreground">
            A full course: lectures, notes, problem sets with solutions and exams.
          </p>
        </button>
      </div>
      <p className="text-xs text-muted-foreground">
        NPTEL, OpenStax, LibreTexts and Open Yale Courses are in{" "}
        <Link to="/search" className="text-primary hover:underline">
          Discover
        </Link>
        .
      </p>
      {source === "ocw" && <OcwDiscover />}
      <Button variant="ghost" onClick={onSkip}>
        Skip, I'll add one later
      </Button>
      <ImportPlaylistDialog open={importOpen} onOpenChange={setImportOpen} />
    </div>
  );
}
