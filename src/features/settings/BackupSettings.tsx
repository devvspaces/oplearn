import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ask, open as openDialog, save } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { ArchiveRestore, DatabaseBackup, FileJson, FolderOpen } from "lucide-react";
import type { BackupInfo } from "@/bindings";
import { commands, errorMessage, useAppInfo } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/misc";
import { formatBytes } from "@/features/downloads/useDownloads";

const KIND: Record<BackupInfo["kind"], string> = {
  daily: "Daily",
  manual: "Manual",
  safety: "Before restore",
};

/** Settings → Backups: daily/manual database backups, restore, and JSON export/import. */
export function BackupSettings() {
  const qc = useQueryClient();
  const { data: info } = useAppInfo();
  const list = useQuery({ queryKey: ["backups"], queryFn: commands.listBackups });
  const [busy, setBusy] = useState(false);

  async function run<T>(f: () => Promise<T>, ok: (v: T) => string) {
    setBusy(true);
    try {
      const v = await f();
      toast.success(ok(v));
      void list.refetch();
      return v;
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  /** Everything may have changed: refetch every query. */
  const reloadAll = () => void qc.invalidateQueries();

  async function restore(path: string, label: string) {
    const yes = await ask(
      `Replace your library with the backup from ${label}? Anything you changed since then will be undone. A backup of your current library is made first, so you can go back.`,
      { title: "Restore backup", kind: "warning", okLabel: "Restore", cancelLabel: "Cancel" },
    );
    if (!yes) return;
    const r = await run(
      () => commands.restoreBackup(path),
      () => "Library restored",
    );
    if (r !== undefined) reloadAll();
  }

  async function restoreFromFile() {
    const f = await openDialog({ title: "Choose a Lectern backup", filters: [{ name: "Backup", extensions: ["db"] }] });
    if (typeof f === "string") await restore(f, f.split(/[\\/]/).pop() ?? "that file");
  }

  async function exportJson() {
    const path = await save({
      title: "Export library",
      defaultPath: `lectern-export-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (path) await run(() => commands.exportLibraryJson(path), () => "Library exported");
  }

  async function importJson() {
    const f = await openDialog({ title: "Import a Lectern export", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (typeof f !== "string") return;
    const yes = await ask(
      "Replace your whole library with this export? A backup of your current library is made first.",
      { title: "Import library", kind: "warning", okLabel: "Replace library", cancelLabel: "Cancel" },
    );
    if (!yes) return;
    const r = await run(
      () => commands.importLibraryJson(f),
      (s) => `Imported ${s.courses} course${s.courses === 1 ? "" : "s"} (${s.rows.toLocaleString()} records)`,
    );
    if (r) reloadAll();
  }

  const backups = list.data ?? [];
  return (
    <div className="space-y-4 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          disabled={busy}
          onClick={() => void run(commands.backupNow, (b) => `Backed up (${formatBytes(b.sizeBytes ?? 0)})`)}
        >
          <DatabaseBackup /> Back up now
        </Button>
        <Button variant="ghost" disabled={busy} onClick={() => void restoreFromFile()}>
          <ArchiveRestore /> Restore from file…
        </Button>
        <span className="text-xs text-muted-foreground">
          {backups[0] ? `Last backup ${backups[0].createdAt}` : "No backups yet"} · a daily backup is kept for 7 days
        </span>
      </div>
      {backups.length > 0 && (
        <ul className="max-h-56 divide-y overflow-y-auto rounded-lg border" aria-label="Backups">
          {backups.map((b) => (
            <li key={b.path} className="flex items-center gap-3 px-3 py-1.5 text-xs">
              <span className="w-32 tabular-nums">{b.createdAt}</span>
              <Badge tone={b.kind === "daily" ? "muted" : "primary"}>{KIND[b.kind]}</Badge>
              <span className="flex-1 text-muted-foreground">{formatBytes(b.sizeBytes ?? 0)}</span>
              <Button
                size="sm"
                variant="ghost"
                className="h-6 px-2 text-xs"
                disabled={busy}
                onClick={() => void restore(b.path, b.createdAt)}
              >
                Restore…
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="rounded-lg border bg-muted/30 p-3">
        <p className="font-medium">Export everything as JSON</p>
        <p className="mt-1 text-xs text-muted-foreground">
          Courses, notes, cards, quizzes, plans, progress and PDF text, in one portable file. Downloaded videos,
          PDFs and attachments stay in your data folder; copy that folder too when moving to another computer.
        </p>
        <div className="mt-2 flex flex-wrap gap-2">
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void exportJson()}>
            <FileJson /> Export library…
          </Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => void importJson()}>
            Import (replace library)…
          </Button>
          {info?.dataDir && (
            <Button size="sm" variant="ghost" onClick={() => void openPath(info.dataDir)}>
              <FolderOpen /> Reveal data folder
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
