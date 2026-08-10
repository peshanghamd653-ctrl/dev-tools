/**
 * What the address bar accepts vs. what `Url::parse` (Rust side) actually
 * needs: a bare `localhost:3000` parses *successfully* as a URL whose
 * scheme is `"localhost"`, not as an http host and port — nothing loads,
 * and nothing errors either, because a webview asked for a `localhost:`
 * page just shows blank. This is the one place that gap gets closed, before
 * the string ever reaches an IPC call.
 */
export function normalizeUrl(input: string): string {
  const trimmed = input.trim();
  return trimmed.includes("://") ? trimmed : `http://${trimmed}`;
}
