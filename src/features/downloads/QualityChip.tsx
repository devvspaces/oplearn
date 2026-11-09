import { HardDrive } from "lucide-react";
import { cn } from "@/lib/utils";

/** "720p" chip for a downloaded lesson; `offline` adds the drive icon and "Offline". */
export function QualityChip({
  quality,
  offline,
  className,
}: {
  quality: string | null | undefined;
  offline?: boolean;
  className?: string;
}) {
  if (!quality && !offline) return null;
  return (
    <span
      title={quality ? `Downloaded at ${quality}: plays offline` : "Downloaded: plays offline"}
      className={cn(
        "inline-flex shrink-0 items-center gap-1 rounded border border-success/40 bg-success/10 px-1.5 py-px text-[10px] font-medium tabular-nums text-success",
        className,
      )}
    >
      {offline && <HardDrive className="size-3" aria-hidden />}
      {quality ?? "Offline"}
      {offline && quality && <span className="sr-only">, offline</span>}
    </span>
  );
}
