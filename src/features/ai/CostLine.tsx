import { AlertTriangle } from "lucide-react";
import type { AiEstimate } from "@/bindings";
import { Spinner } from "@/components/ui/misc";

export function money(usd: number | null | undefined) {
  const v = usd ?? 0;
  return v < 0.01 ? "< $0.01" : `$${v.toFixed(2)}`;
}

/** True when this call would exceed the monthly budget (the backend refuses it too). */
export function overBudget(e: AiEstimate | undefined) {
  return !!e && e.limitUsd != null && (e.monthSpentUsd ?? 0) + (e.costUsd ?? 0) > e.limitUsd;
}

/** "~N tokens · estimated $X with <model>" plus the monthly budget status. */
export function CostLine({ estimate, what }: { estimate: AiEstimate | undefined; what?: string }) {
  if (!estimate) return <Spinner />;
  const spent = estimate.monthSpentUsd ?? 0;
  const limit = estimate.limitUsd;
  const over = overBudget(estimate);
  const near = limit != null && spent + (estimate.costUsd ?? 0) > limit * 0.8;
  return (
    <div className="space-y-1 text-xs">
      <p className="tabular-nums">
        {what ? `${what} · ` : ""}~{(estimate.inputTokens + estimate.outputTokens).toLocaleString()} tokens ·
        estimated <b>{money(estimate.costUsd)}</b> with {estimate.model}
      </p>
      {limit != null && (
        <p
          className={
            over
              ? "flex items-center gap-1 text-destructive"
              : near
                ? "flex items-center gap-1 text-warning"
                : "text-muted-foreground"
          }
        >
          {(near || over) && <AlertTriangle className="size-3.5" />}
          {money(spent)} of your {money(limit)} monthly AI budget used
          {over ? ". This would go over it; raise the budget in Settings → AI." : ""}
        </p>
      )}
    </div>
  );
}
