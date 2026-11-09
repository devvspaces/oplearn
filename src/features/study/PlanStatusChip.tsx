import type { PlanStatus } from "@/bindings";
import { Badge } from "@/components/ui/misc";

export function PlanStatusChip({ status }: { status: PlanStatus | null | undefined }) {
  if (!status) return null;
  if (status.kind === "behind")
    return (
      <Badge tone="warning" title="Planned lessons before today that aren't complete">
        Behind by {status.lessons}
      </Badge>
    );
  if (status.kind === "ahead") return <Badge tone="success">Ahead by {status.lessons}</Badge>;
  return <Badge tone="primary">On track</Badge>;
}
