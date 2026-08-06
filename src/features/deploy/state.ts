import type { DeployProject, Deployment } from "@/shared/ipc/client";

/**
 * Vercel reports `state` as an uppercase string. These are the five the API
 * documents today; `unknown` exists because a sixth will eventually arrive,
 * and a row that renders nothing — or crashes on a missing map entry — is
 * worse than one that renders a word we didn't plan for.
 */
export type DeployState =
  | "ready"
  | "error"
  | "building"
  | "queued"
  | "canceled"
  | "unknown";

const STATES: Record<string, DeployState> = {
  READY: "ready",
  ERROR: "error",
  BUILDING: "building",
  QUEUED: "queued",
  CANCELED: "canceled",
  // Vercel has shipped both spellings over the years; they mean one thing.
  CANCELLED: "canceled",
};

export function deployState(raw: string): DeployState {
  return STATES[raw.trim().toUpperCase()] ?? "unknown";
}

const LABELS: Record<Exclude<DeployState, "unknown">, string> = {
  ready: "Ready",
  error: "Error",
  building: "Building",
  queued: "Queued",
  canceled: "Canceled",
};

/**
 * The text on the badge. Colour alone can't carry state, so every badge says
 * its state in words — including states we've never seen, which are shown as
 * whatever Vercel called them rather than swallowed into "Unknown".
 */
export function deployStateLabel(raw: string): string {
  const state = deployState(raw);
  return state === "unknown" ? humanize(raw) : LABELS[state];
}

/** `SOME_NEW_STATE` -> `Some new state`. Never empty — a blank badge says nothing. */
function humanize(raw: string): string {
  const words = raw.trim().replace(/[_-]+/g, " ").toLowerCase();
  if (words === "") return "Unknown";
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * Vercel returns deployment URLs as bare hostnames (`app-a1b2c3.vercel.app`),
 * so a link needs the scheme put back. Two things are refused afterwards,
 * because this string arrives from a remote API and is about to be handed to
 * the OS's default handler: any scheme other than http(s), so we never launch
 * `file:` or some other protocol on Vercel's say-so; and any URL carrying
 * embedded credentials, which is the shape `mailto:you@example.com` collapses
 * into once a scheme is prepended and the shape a phishing link wears.
 */
export function deploymentHref(url: string): string | null {
  const raw = url.trim();
  if (raw === "") return null;
  const candidate = /^[a-z][a-z0-9+.-]*:\/\//i.test(raw) ? raw : `https://${raw}`;
  try {
    const parsed = new URL(candidate);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    if (parsed.username !== "" || parsed.password !== "") return null;
    return parsed.toString();
  } catch {
    return null;
  }
}

/** The hostname on its own — the scheme is noise in a list of forty rows. */
export function displayUrl(url: string): string {
  return url.trim().replace(/^https?:\/\//i, "").replace(/\/+$/, "");
}

/**
 * First line only, and only when there is one. Most deployments carry no
 * commit message at all (CLI deploys, redeploys), so this returns null rather
 * than an empty string — the caller drops the element instead of rendering a
 * blank gap where text should be.
 */
export function commitSummary(message: string | null): string | null {
  if (message === null) return null;
  const first = message.split("\n")[0]?.trim() ?? "";
  return first === "" ? null : first;
}

/** Vercel leaves `target` null for preview builds rather than saying so. */
export function targetLabel(target: string | null): string {
  const raw = target?.trim() ?? "";
  if (raw === "") return "Preview";
  return raw.charAt(0).toUpperCase() + raw.slice(1).toLowerCase();
}

export function timeAgo(ms: number, now: number = Date.now()): string {
  if (!Number.isFinite(ms)) return "—";
  const seconds = Math.floor((now - ms) / 1000);
  // Clock skew between this machine and Vercel can put a deployment a few
  // seconds in the future; "in -3s" would be worse than rounding to now.
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

/** Newest first, without mutating the query cache's array. */
export function sortProjects(projects: DeployProject[]): DeployProject[] {
  return [...projects].sort((a, b) => b.updatedAt - a.updatedAt);
}

/** Newest first, without mutating the query cache's array. */
export function sortDeployments(deployments: Deployment[]): Deployment[] {
  return [...deployments].sort((a, b) => b.createdAt - a.createdAt);
}

/**
 * "No token stored" and "the stored token was rejected" collapse into the same
 * blank screen if you don't separate them, and they need opposite things from
 * the user — add a credential vs. replace one that looks fine and isn't.
 *
 * The backend keeps them apart at the type level (`DeployError::NotConfigured`
 * vs `DeployError::Auth`), but Tauri flattens both to a string on the way
 * across, so this reads them back:
 *
 *   Auth          -> "Vercel rejected the token (HTTP 401)"
 *   NotConfigured -> "no Vercel token configured"
 *
 * The extra patterns are slack for a future rewording: a renamed message
 * should land on a near-enough screen rather than on "something went wrong".
 * The status match is anchored to the `(HTTP nnn)` the backend actually
 * formats, so a `403` appearing inside some other error's response body
 * doesn't get mistaken for a rejected token.
 */
export type DeployFailure = "auth" | "unconfigured" | "other";

export function classifyDeployError(error: unknown): DeployFailure {
  const text = String(error).toLowerCase();
  if (
    text.includes("rejected the token") ||
    text.includes("unauthorized") ||
    text.includes("forbidden") ||
    /\(http (401|403)\)/.test(text)
  ) {
    return "auth";
  }
  if (
    text.includes("token configured") ||
    text.includes("not configured") ||
    text.includes("no token") ||
    text.includes("missing token")
  ) {
    return "unconfigured";
  }
  return "other";
}
