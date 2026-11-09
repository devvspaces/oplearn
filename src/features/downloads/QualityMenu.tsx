import { useQuery } from "@tanstack/react-query";
import type { Quality } from "@/bindings";
import { commands } from "@/lib/api";
import { DropdownMenuItem, DropdownMenuLabel } from "@/components/ui/dropdown-menu";
import { estimateBytes, QUALITIES } from "./quality";
import { formatBytes } from "./useDownloads";

/** Quality choices for one lesson (used inside a dropdown or submenu). */
export function QualityMenuItems({
  durationSec,
  onPick,
}: {
  durationSec: number | null | undefined;
  onPick: (q: Quality) => void;
}) {
  const { data: settings } = useQuery({
    queryKey: ["downloadSettings"],
    queryFn: commands.getDownloadSettings,
  });
  return (
    <>
      <DropdownMenuLabel>Quality · size is an estimate</DropdownMenuLabel>
      {QUALITIES.map((q) => {
        const bytes = estimateBytes(durationSec, q);
        return (
          <DropdownMenuItem key={q} onSelect={() => onPick(q)}>
            <span className="w-12 font-medium tabular-nums">{q}</span>
            {settings?.quality === q && <span className="text-[10px] text-primary">default</span>}
            <span className="ml-auto pl-4 text-xs tabular-nums text-muted-foreground">
              {bytes ? `~${formatBytes(bytes)}` : ""}
            </span>
          </DropdownMenuItem>
        );
      })}
    </>
  );
}
