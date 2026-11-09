import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { LogIn, LogOut } from "lucide-react";
import type { GoogleAccount } from "@/bindings";
import { commands, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input, Label } from "@/components/ui/input";
import { Badge, Progress } from "@/components/ui/misc";

/** Settings → Accounts: bring-your-own Google OAuth client, sign in/out, and API quota. */
export function AccountSettings() {
  const qc = useQueryClient();
  const { data: acct } = useQuery({ queryKey: ["googleAccount"], queryFn: commands.getGoogleAccount });
  const { data: quota } = useQuery({
    queryKey: ["quota"],
    queryFn: commands.getQuota,
    refetchInterval: 60_000,
  });
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [editing, setEditing] = useState(false);
  const set = (a: GoogleAccount) => {
    qc.setQueryData(["googleAccount"], a);
    void qc.invalidateQueries({ queryKey: ["myPlaylists"] });
  };
  const saveClient = useMutation({
    mutationFn: () => commands.setGoogleClient(clientId, secret || null),
    onSuccess: (a) => {
      set(a);
      setEditing(false);
      setSecret("");
      toast.success("Google client saved");
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  const signIn = useMutation({
    mutationFn: commands.googleSignIn,
    onSuccess: (a) => {
      set(a);
      toast.success(a.channelTitle ? `Signed in as ${a.channelTitle}` : "Signed in with Google");
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  const signOut = useMutation({
    mutationFn: commands.googleSignOut,
    onSuccess: set,
    onError: (e) => toast.error(errorMessage(e)),
  });

  const showForm = editing || !acct?.configured;
  return (
    <div className="space-y-5 text-sm">
      {quota && (
        <div>
          <div className="mb-1 flex justify-between text-xs text-muted-foreground">
            <span>YouTube Data API quota today (estimate)</span>
            <span className="tabular-nums">
              ~{Math.max(0, quota.limit - quota.used).toLocaleString()} of {quota.limit.toLocaleString()}{" "}
              units left
            </span>
          </div>
          <Progress value={(100 * quota.used) / Math.max(1, quota.limit)} label="Quota used" />
          <p className="mt-1 text-[11px] text-muted-foreground">
            A search costs 100 units; imports cost about 1 per 50 videos. Resets at midnight Pacific. Searches
            fall back to yt-dlp when it runs out.
          </p>
        </div>
      )}

      <div>
        <p className="mb-1 font-medium">Google sign-in</p>
        <p className="mb-3 text-xs text-muted-foreground">
          Lets Lectern list your own YouTube playlists (read-only). Lectern uses your own OAuth client, so
          nothing passes through a third party.{" "}
          <button
            className="text-primary hover:underline"
            onClick={() => void openUrl("https://console.cloud.google.com/apis/credentials")}
          >
            Create one in Google Cloud
          </button>
          : enable "YouTube Data API v3", configure the consent screen (External, add yourself as a test
          user), then Create credentials → OAuth client ID → <b>Desktop app</b>.
        </p>
        {showForm ? (
          <form
            className="grid max-w-lg gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              saveClient.mutate();
            }}
          >
            <div className="grid gap-1">
              <Label htmlFor="g-client">Client ID</Label>
              <Input
                id="g-client"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="1234-abc.apps.googleusercontent.com"
              />
            </div>
            <div className="grid gap-1">
              <Label htmlFor="g-secret">Client secret (stored in your keychain)</Label>
              <Input
                id="g-secret"
                type="password"
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
                placeholder="GOCSPX-…"
              />
            </div>
            <div className="flex gap-2">
              <Button type="submit" size="sm" disabled={!clientId.trim() || saveClient.isPending}>
                Save client
              </Button>
              {acct?.configured && (
                <Button type="button" size="sm" variant="ghost" onClick={() => setEditing(false)}>
                  Cancel
                </Button>
              )}
            </div>
          </form>
        ) : (
          <div className="flex flex-wrap items-center gap-2">
            {acct?.signedIn ? (
              <>
                <Badge tone="success">Signed in{acct.channelTitle ? ` as ${acct.channelTitle}` : ""}</Badge>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={signOut.isPending}
                  onClick={() => signOut.mutate()}
                >
                  <LogOut /> Sign out & revoke
                </Button>
              </>
            ) : (
              <Button size="sm" disabled={signIn.isPending} onClick={() => signIn.mutate()}>
                <LogIn /> {signIn.isPending ? "Waiting for the browser…" : "Sign in with Google"}
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setClientId(acct?.clientId ?? "");
                setEditing(true);
              }}
            >
              Change client
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
