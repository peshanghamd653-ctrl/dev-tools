/**
 * Update-check state and formatting. No Tauri imports, so the state machine is
 * testable without a shell.
 *
 * The security model is worth stating where someone will read it: the update
 * *manifest* is signed with a minisign key whose public half is compiled into
 * the app (`tauri.conf.json`). Tauri verifies that signature before it will
 * install anything, so a compromised release host — or anyone who can answer
 * for the endpoint — still cannot hand this app a payload it accepts. TLS
 * protects the transport; the signature is what protects the user.
 */

/** Where the flow is. One value, so the UI cannot render two states at once. */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate"; checkedAt: number }
  | { kind: "available"; version: string; notes?: string; date?: string }
  | { kind: "downloading"; version: string; downloaded: number; total?: number }
  | { kind: "installing"; version: string }
  | { kind: "failed"; message: string };

/**
 * Progress as a 0–1 fraction, or `null` when it cannot be known.
 *
 * The server may omit `Content-Length`, in which case `total` is undefined and
 * an honest indeterminate bar beats a fabricated percentage that sticks at
 * some arbitrary number.
 */
export function downloadFraction(
  downloaded: number,
  total: number | undefined,
): number | null {
  if (!total || total <= 0 || !Number.isFinite(total)) return null;
  if (!Number.isFinite(downloaded) || downloaded < 0) return 0;
  return Math.min(1, downloaded / total);
}

/** `12.4 MB`, or bytes below a megabyte. Small local helper — updates are the
 * only place showing a download size, and the shared byte formatter rounds for
 * file listings rather than progress. */
export function formatTransferred(
  downloaded: number,
  total: number | undefined,
): string {
  const mb = (n: number) => `${(n / 1_048_576).toFixed(1)} MB`;
  return total ? `${mb(downloaded)} of ${mb(total)}` : mb(downloaded);
}

/**
 * What to say about an update failure.
 *
 * Deliberately does not surface the raw error for the two cases a user can
 * act on. Everything else passes through verbatim rather than being flattened
 * into "something went wrong", which tells nobody anything.
 */
export function describeUpdateError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  const lower = raw.toLowerCase();

  // The endpoint is unreachable or the release has no manifest yet. Both look
  // identical to a user and have the same answer: it is not their machine.
  if (
    lower.includes("network") ||
    lower.includes("failed to fetch") ||
    lower.includes("dns") ||
    lower.includes("connect")
  ) {
    return "Could not reach the update server. Check your connection and try again.";
  }
  // A signature mismatch is the one failure that must never be shrugged off:
  // it means the manifest was not signed by the key this build trusts.
  if (lower.includes("signature") || lower.includes("minisign")) {
    return "This update failed its signature check and was not installed. That should not happen — please report it rather than retrying.";
  }
  return raw;
}

/** Whether a check is in flight, for disabling the button without a second flag. */
export function isBusy(state: UpdateState): boolean {
  return (
    state.kind === "checking" ||
    state.kind === "downloading" ||
    state.kind === "installing"
  );
}
