import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Sparkles } from "lucide-react";
import type { AiStatus } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge, Progress } from "@/components/ui/misc";
import { money } from "./CostLine";

export const MODELS = [
  ["claude-opus-5-5", "Claude Opus 5.5 (best quality)"],
  ["claude-sonnet-5-5", "Claude Sonnet 5.5 (about half the cost)"],
] as const;

export const useAiStatus = () => useQuery({ queryKey: ["aiStatus"], queryFn: commands.getAiStatus });

/** Settings → AI: off by default; your own Anthropic key in the keychain. */
export function AiSettings() {
  const qc = useQueryClient();
  const { data: s } = useAiStatus();
  const [key, setKey] = useState("");
  const set = (v: AiStatus) => qc.setQueryData(["aiStatus"], v);
  const save = useMutation({
    mutationFn: commands.setAiSettings,
    onSuccess: set,
    onError: (e) => toast.error(errorMessage(e)),
  });
  const saveKey = useMutation({
    mutationFn: (k: string | null) => commands.setAiKey(k),
    onSuccess: (v) => {
      set(v);
      setKey("");
      toast.success(v.hasKey ? "API key saved to your keychain" : "API key removed");
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  if (!s) return null;
  return (
    <div className="space-y-4 text-sm">
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={s.settings.enabled}
          onChange={(e) => save.mutate({ ...s.settings, enabled: e.target.checked })}
        />
        <Sparkles className="size-4 text-primary" /> Enable AI features
      </label>
      <p className="text-xs text-muted-foreground">
        Lectern only sends data to Anthropic when you click an AI action, and shows the estimated cost first.
        Results are labelled as AI-generated.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        {s.hasKey ? <Badge tone="success">API key saved</Badge> : <Badge tone="muted">No API key</Badge>}
        <Input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="sk-ant-…"
          className="h-8 max-w-xs"
          aria-label="Anthropic API key"
        />
        <Button size="sm" disabled={!key.trim() || saveKey.isPending} onClick={() => saveKey.mutate(key)}>
          Save key
        </Button>
        {s.hasKey && (
          <Button size="sm" variant="ghost" onClick={() => saveKey.mutate(null)}>
            Remove
          </Button>
        )}
      </div>
      <BudgetAndUsage
        limit={s.settings.monthlyLimitUsd ?? null}
        onLimit={(v) => save.mutate({ ...s.settings, monthlyLimitUsd: v })}
      />
      <label className="flex items-center gap-2">
        Model
        <select
          value={s.settings.model}
          onChange={(e) => save.mutate({ ...s.settings, model: e.target.value })}
          className="rounded-md border bg-background px-2 py-1 text-xs"
        >
          {MODELS.map(([id, label]) => (
            <option key={id} value={id}>
              {label}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

const FEATURE_LABEL: Record<string, string> = {
  restructure: "Restructure",
  flashcards: "Flashcards",
  quiz: "Quiz",
  plan: "Study plan",
};

function BudgetAndUsage({ limit, onLimit }: { limit: number | null; onLimit: (v: number | null) => void }) {
  const qc = useQueryClient();
  const { data: u } = useQuery({ queryKey: ["aiUsage"], queryFn: commands.getAiUsage });
  const [draft, setDraft] = useState(limit != null ? String(limit) : "");
  const [showAll, setShowAll] = useState(false);
  const clear = useMutation({
    mutationFn: commands.clearAiUsage,
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["aiUsage"] }),
  });
  return (
    <div className="space-y-3 rounded-lg border p-3">
      <div className="grid grid-cols-3 gap-2 text-center">
        {(
          [
            ["Today", u?.todayUsd],
            ["This month", u?.monthUsd],
            ["All time", u?.allTimeUsd],
          ] as const
        ).map(([label, v]) => (
          <div key={label} className="rounded-md bg-muted/50 p-2">
            <p className="text-lg font-semibold tabular-nums">{money(v ?? 0)}</p>
            <p className="text-[11px] text-muted-foreground">{label}</p>
          </div>
        ))}
      </div>
      {limit != null && u && (
        <div>
          <div className="mb-1 flex justify-between text-xs text-muted-foreground">
            <span>Monthly budget</span>
            <span className="tabular-nums">
              {money(u.monthUsd)} of {money(limit)}
            </span>
          </div>
          <Progress
            value={(100 * (u.monthUsd ?? 0)) / Math.max(0.0001, limit)}
            label="Monthly AI budget used"
          />
        </div>
      )}
      <form
        className="flex flex-wrap items-center gap-2 text-sm"
        onSubmit={(e) => {
          e.preventDefault();
          const v = draft.trim() === "" ? null : Math.max(0, Number(draft));
          onLimit(v != null && Number.isFinite(v) ? v : null);
        }}
      >
        <label htmlFor="ai-budget">Monthly limit (USD)</label>
        <Input
          id="ai-budget"
          inputMode="decimal"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="No limit"
          className="h-8 w-28"
        />
        <Button size="sm" type="submit" variant="outline">
          Save limit
        </Button>
        <span className="text-xs text-muted-foreground">
          AI actions are blocked once the month's spending reaches it.
        </span>
      </form>
      {u && u.byFeature.length > 0 && (
        <table className="w-full text-xs">
          <thead className="text-left text-muted-foreground">
            <tr>
              <th className="py-1 font-normal">This month</th>
              <th className="py-1 text-right font-normal">Calls</th>
              <th className="py-1 text-right font-normal">Cost</th>
            </tr>
          </thead>
          <tbody>
            {u.byFeature.map((f) => (
              <tr key={f.feature} className="border-t">
                <td className="py-1">{FEATURE_LABEL[f.feature] ?? f.feature}</td>
                <td className="py-1 text-right tabular-nums">{f.calls}</td>
                <td className="py-1 text-right tabular-nums">{money(f.costUsd)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {u && u.recent.length > 0 && (
        <details open={showAll} onToggle={(e) => setShowAll((e.target as HTMLDetailsElement).open)}>
          <summary className="cursor-pointer text-xs text-muted-foreground">
            Recent calls ({u.recent.length})
          </summary>
          <ul className="mt-2 max-h-48 space-y-0.5 overflow-y-auto text-[11px] tabular-nums">
            {u.recent.map((r, i) => (
              <li key={i} className="flex gap-2">
                <span className="w-32 shrink-0 text-muted-foreground">{new Date(r.at).toLocaleString()}</span>
                <span className="w-24 shrink-0">{FEATURE_LABEL[r.feature] ?? r.feature}</span>
                <span className="min-w-0 flex-1 truncate text-muted-foreground">
                  {r.model} · {(r.inputTokens + r.outputTokens).toLocaleString()} tokens
                  {r.ok ? "" : " · failed"}
                </span>
                <span>{money(r.costUsd)}</span>
              </li>
            ))}
          </ul>
          <Button size="sm" variant="ghost" className="mt-1" onClick={() => clear.mutate()}>
            Clear history
          </Button>
        </details>
      )}
    </div>
  );
}
