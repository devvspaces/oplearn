import { useState } from "react";
import { Link, createFileRoute } from "@tanstack/react-router";
import type { DownloadSettings } from "@/bindings";
import { formatBytes } from "@/features/downloads/useDownloads";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog, message } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { KeyRound } from "lucide-react";
import { commands, errorMessage, qk, useAppInfo } from "@/lib/api";
import { useUi, type Theme } from "@/stores/ui";
import { Button } from "@/components/ui/button";
import { Input, Label } from "@/components/ui/input";
import { Badge } from "@/components/ui/misc";
import { cn } from "@/lib/utils";
import { StudySettingsForm } from "@/features/study/StudySettingsForm";
import { AccountSettings } from "@/features/youtube/AccountSettings";
import { AiSettings } from "@/features/ai/AiSettings";
import { BackupSettings } from "@/features/settings/BackupSettings";

export const Route = createFileRoute("/settings")({ component: SettingsPage });

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-xl border bg-card p-5">
      <h2 className="text-sm font-semibold">{title}</h2>
      {description && <p className="mt-1 text-xs text-muted-foreground">{description}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

function SettingsPage() {
  const { data: info } = useAppInfo();
  const qc = useQueryClient();
  const ytdlp = useQuery({
    queryKey: qk.ytdlpVersion,
    queryFn: commands.getYtdlpVersion,
    retry: false,
    staleTime: Infinity,
  });
  const { theme, setTheme, pauseOnNote, setPauseOnNote } = useUi();
  const [updating, setUpdating] = useState(false);
  const dl = useQuery({ queryKey: ["downloadSettings"], queryFn: commands.getDownloadSettings });
  const storage = useQuery({ queryKey: ["storage"], queryFn: commands.getStorageUsage });
  const saveDl = async (patch: Partial<DownloadSettings>) => {
    if (!dl.data) return;
    try {
      await commands.setDownloadSettings({ ...dl.data, ...patch });
      void dl.refetch();
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };
  const subLangs = useQuery({ queryKey: ["subtitleLangs"], queryFn: commands.getSubtitleLangs });
  const [key, setKey] = useState("");

  async function saveKey() {
    try {
      await commands.setApiKey(key);
      setKey("");
      toast.success("API key saved to your system keychain");
      void qc.invalidateQueries({ queryKey: qk.appInfo });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  async function chooseDataDir() {
    const dir = await openDialog({ directory: true, title: "Choose Lectern data folder" });
    if (typeof dir !== "string") return;
    try {
      await commands.setDataDir(dir);
      await message(
        `Lectern will use ${dir} after you restart the app. Existing data is not moved automatically.`,
        {
          title: "Restart required",
        },
      );
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  return (
    <div className="mx-auto max-w-3xl space-y-4 p-8">
      <h1 className="text-2xl font-semibold">Settings</h1>

      <Section title="Appearance">
        <div role="radiogroup" aria-label="Theme" className="flex gap-1 rounded-lg bg-muted p-1 w-fit">
          {(["dark", "light", "system"] as Theme[]).map((t) => (
            <button
              key={t}
              role="radio"
              aria-checked={theme === t}
              onClick={() => setTheme(t)}
              className={cn(
                "rounded-md px-3 py-1 text-xs font-medium capitalize text-muted-foreground",
                theme === t && "bg-background text-foreground shadow-sm",
              )}
            >
              {t}
            </button>
          ))}
        </div>
      </Section>

      <Section
        title="YouTube Data API key"
        description="Optional. With a key, imports are faster and include each video's license. Without one, Lectern uses yt-dlp. The key is stored in your OS keychain, never on disk."
      >
        <div className="flex items-center gap-2">
          {info?.hasApiKey ? (
            <Badge tone="success">
              <KeyRound className="size-3" /> Key saved
            </Badge>
          ) : (
            <Badge tone="muted">No key</Badge>
          )}
          <button
            className="text-xs text-primary hover:underline"
            onClick={() => void openUrl("https://developers.google.com/youtube/v3/getting-started")}
          >
            How to get a key
          </button>
        </div>
        <div className="mt-3 flex gap-2">
          <Label htmlFor="api-key" className="sr-only">
            API key
          </Label>
          <Input
            id="api-key"
            type="password"
            autoComplete="off"
            placeholder="AIza…"
            value={key}
            onChange={(e) => setKey(e.target.value)}
          />
          <Button onClick={saveKey} disabled={!key.trim()}>
            Save
          </Button>
          {info?.hasApiKey && (
            <Button
              variant="ghost"
              onClick={async () => {
                await commands.clearApiKey();
                void qc.invalidateQueries({ queryKey: qk.appInfo });
                toast.success("API key removed");
              }}
            >
              Remove
            </Button>
          )}
        </div>
      </Section>

      <Section title="Studying" description="How the player behaves while you take notes.">
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={pauseOnNote} onChange={(e) => setPauseOnNote(e.target.checked)} />
          Pause the video when I create a note, question or to-do
        </label>
        <div className="mt-4 flex items-center gap-2">
          <Label htmlFor="sub-langs" className="shrink-0 text-xs font-normal text-muted-foreground">
            Transcript languages
          </Label>
          <Input
            id="sub-langs"
            className="h-8 max-w-60 text-sm"
            defaultValue={subLangs.data?.join(", ") ?? ""}
            key={subLangs.data?.join(",")}
            placeholder="en"
            onBlur={async (e) => {
              const langs = e.target.value
                .split(",")
                .map((s) => s.trim())
                .filter(Boolean);
              try {
                await commands.setSubtitleLangs(langs);
                void subLangs.refetch();
              } catch (err) {
                toast.error(errorMessage(err));
              }
            }}
          />
          <span className="text-xs text-muted-foreground">in priority order, e.g. “en, fr”</span>
        </div>
      </Section>

      <Section
        title="Data folder"
        description="Your library database and any downloaded media live here. Point it at an external drive if you like."
      >
        <div className="flex items-center gap-3">
          <code className="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1.5 text-xs">
            {info?.dataDir}
          </code>
          <Button variant="outline" onClick={chooseDataDir}>
            Change…
          </Button>
        </div>
      </Section>

      <Section
        title="Backups"
        description="Lectern backs up your library database every day. Restore any backup, or export everything to a single file."
      >
        <BackupSettings />
      </Section>

      <Section
        title="yt-dlp"
        description="Used for imports without an API key, transcripts and downloads. Lectern checks for a new version once a day."
      >
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
          <dt className="text-muted-foreground">Version</dt>
          <dd>
            {ytdlp.isLoading
              ? "checking…"
              : (ytdlp.data ?? <span className="text-destructive">{errorMessage(ytdlp.error)}</span>)}
          </dd>
          <dt className="text-muted-foreground">Binary</dt>
          <dd className="truncate">{info?.ytdlpPath ?? "not found"}</dd>
        </dl>
        <Button
          variant="outline"
          size="sm"
          className="mt-3"
          disabled={updating}
          onClick={async () => {
            setUpdating(true);
            try {
              const r = await commands.checkYtdlpUpdate();
              toast.success(
                r.updated
                  ? `Updated yt-dlp to ${r.latest}`
                  : `yt-dlp is up to date (${r.latest ?? r.current})`,
              );
              void ytdlp.refetch();
              void qc.invalidateQueries({ queryKey: qk.appInfo });
            } catch (e) {
              toast.error(errorMessage(e));
            } finally {
              setUpdating(false);
            }
          }}
        >
          {updating ? "Checking…" : "Check for update"}
        </Button>
      </Section>

      <Section
        title="Study"
        description="Your daily goal, study days and reminders drive the Today screen, streaks and course plans."
      >
        <StudySettingsForm />
      </Section>

      <Section
        title="Downloads"
        description="Used when a course is in Download mode. H.264/AAC MP4 is preferred so files play everywhere."
      >
        {dl.data && (
          <div className="flex flex-wrap items-center gap-6 text-sm">
            <label className="flex items-center gap-2">
              Default quality
              <select
                value={dl.data.quality}
                onChange={(e) => void saveDl({ quality: e.target.value as DownloadSettings["quality"] })}
                className="rounded-md border bg-background px-2 py-1 text-xs"
              >
                {["360p", "480p", "720p", "1080p"].map((q) => (
                  <option key={q}>{q}</option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-2">
              Simultaneous downloads
              <select
                value={dl.data.concurrency}
                onChange={(e) => void saveDl({ concurrency: Number(e.target.value) })}
                className="rounded-md border bg-background px-2 py-1 text-xs"
              >
                {[1, 2, 3, 4].map((n) => (
                  <option key={n}>{n}</option>
                ))}
              </select>
            </label>
          </div>
        )}
        <p className="mt-3 text-xs text-muted-foreground">
          Using {formatBytes(storage.data?.totalBytes) || "0 B"} for downloaded media.{" "}
          <Link to="/downloads" className="text-primary hover:underline">
            Manage downloads
          </Link>
        </p>
      </Section>

      <Section
        title="Accounts"
        description="YouTube search quota and Google sign-in for importing your own playlists."
      >
        <AccountSettings />
      </Section>

      <Section title="AI" description="Optional. Uses your own Anthropic API key.">
        <AiSettings />
      </Section>

      <Section title="Limitations">
        <ul className="list-disc space-y-1 pl-4 text-xs text-muted-foreground">
          <li>The YouTube API does not expose Watch Later or watch history.</li>
          <li>YouTube Premium offline downloads cannot be used by third-party apps.</li>
          <li>
            Lectern streams through YouTube's official embedded player. Downloading is opt-in per course, and
            you are responsible for only downloading content you have the right to.
          </li>
        </ul>
      </Section>

      <p className="text-center text-xs text-muted-foreground">Lectern {info?.version}</p>
    </div>
  );
}
