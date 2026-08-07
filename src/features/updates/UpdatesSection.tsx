import { useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { Download, RefreshCw, ShieldCheck } from "lucide-react";

import { useAppInfo } from "@/features/app/hooks";
import { isDesktopShell } from "@/shared/ipc/client";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  describeUpdateError,
  downloadFraction,
  formatTransferred,
  isBusy,
  type UpdateState,
} from "./updates";

/**
 * Self-update, as a Settings card.
 *
 * Checking is manual rather than automatic on boot. An app that phones a
 * server every launch is doing something the user did not ask for, and this
 * one's whole position is that it does not — there is no telemetry anywhere
 * else, so a silent update ping would be the exception that undoes the claim.
 */
export function UpdatesSection() {
  const { data: info } = useAppInfo();
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  async function checkForUpdate() {
    setState({ kind: "checking" });
    try {
      const update = await check();
      if (!update) {
        setState({ kind: "upToDate", checkedAt: Date.now() });
        return;
      }
      setState({
        kind: "available",
        version: update.version,
        notes: update.body,
        date: update.date,
      });
    } catch (error) {
      setState({ kind: "failed", message: describeUpdateError(error) });
    }
  }

  async function install() {
    setState({ kind: "checking" });
    try {
      // Re-check rather than holding the handle from the earlier call: the
      // returned Update owns a live connection, and the gap between "user saw
      // the notes" and "user pressed install" is unbounded.
      const update = await check();
      if (!update) {
        setState({ kind: "upToDate", checkedAt: Date.now() });
        return;
      }

      let downloaded = 0;
      let total: number | undefined;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
          setState({ kind: "downloading", version: update.version, downloaded: 0, total });
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setState({ kind: "downloading", version: update.version, downloaded, total });
        } else if (event.event === "Finished") {
          setState({ kind: "installing", version: update.version });
        }
      });
      // On Windows the NSIS installer terminates this process and restarts it,
      // so anything after this line usually never runs. It is here for the
      // case where it does.
      setState({ kind: "installing", version: update.version });
    } catch (error) {
      setState({ kind: "failed", message: describeUpdateError(error) });
    }
  }

  if (!isDesktopShell()) {
    return (
      <Card>
        <CardContent className="space-y-1 py-6">
          <p className="font-medium">Updates</p>
          <p className="text-sm text-muted-foreground">
            Updating is handled by the desktop shell — a browser tab has nothing
            to replace.
          </p>
        </CardContent>
      </Card>
    );
  }

  const fraction =
    state.kind === "downloading"
      ? downloadFraction(state.downloaded, state.total)
      : null;

  return (
    <Card>
      <CardContent className="space-y-4 py-6">
        <div className="flex items-start justify-between gap-4">
          <div className="space-y-1">
            <p className="font-medium">Updates</p>
            <p className="text-sm text-muted-foreground">
              You are on version{" "}
              <span className="text-foreground">{info?.version ?? "—"}</span>.
              DevOS only contacts the update server when you ask it to.
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            className="shrink-0 gap-1.5"
            disabled={isBusy(state)}
            onClick={() => void checkForUpdate()}
          >
            <RefreshCw
              className={`size-4 ${state.kind === "checking" ? "animate-spin" : ""}`}
            />
            {state.kind === "checking" ? "Checking…" : "Check for updates"}
          </Button>
        </div>

        {state.kind === "upToDate" && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <ShieldCheck className="size-4 text-emerald-400" />
            You are running the latest version.
          </p>
        )}

        {state.kind === "available" && (
          <div className="space-y-3 rounded-md border border-primary/40 bg-primary/5 p-3">
            <div>
              <p className="text-sm font-medium">Version {state.version} is available</p>
              {state.date && (
                <p className="text-xs text-muted-foreground">Released {state.date}</p>
              )}
            </div>
            {state.notes && (
              <pre className="max-h-40 overflow-auto whitespace-pre-wrap font-sans text-xs text-muted-foreground">
                {state.notes}
              </pre>
            )}
            <Button size="sm" className="gap-1.5" onClick={() => void install()}>
              <Download className="size-4" />
              Download and install
            </Button>
            <p className="text-xs text-muted-foreground">
              DevOS will close while the installer runs, then reopen.
            </p>
          </div>
        )}

        {state.kind === "downloading" && (
          <div className="space-y-2">
            <p className="text-sm text-muted-foreground">
              Downloading {state.version} — {formatTransferred(state.downloaded, state.total)}
            </p>
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
              <div
                className={`h-full bg-primary ${fraction === null ? "w-1/3 animate-pulse" : ""}`}
                style={fraction === null ? undefined : { width: `${fraction * 100}%` }}
              />
            </div>
          </div>
        )}

        {state.kind === "installing" && (
          <p className="text-sm text-muted-foreground">
            Installing {state.version}. DevOS will close and reopen.
          </p>
        )}

        {state.kind === "failed" && (
          <p className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm">
            {state.message}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
