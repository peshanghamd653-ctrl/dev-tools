/**
 * Everything about the screenshot → issue flow that can be decided without a
 * DOM: annotation geometry, the system-context block, error classification and
 * the repository choices. The canvas itself is untestable pixel work and stays
 * in `ShotAnnotator.tsx`; the arithmetic that positions the rectangles lives
 * here so a drag that goes up-and-left can be proven not to produce a
 * negative-width redaction that covers nothing.
 */
import type { IssueTarget } from "@/shared/ipc/client";

/** The secret DevOS reads to talk to GitHub. Named here so the copy can say it. */
export const GITHUB_TOKEN_SECRET = "github_token";

/* -------------------------------------------------------------------------
 * Annotation geometry
 * ---------------------------------------------------------------------- */

export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

/**
 * `redact` is destructive and opaque; `highlight` is a hollow ring that adds
 * nothing to the pixels underneath. The distinction matters beyond drawing:
 * only a redaction makes the original capture unsafe to hand out (see
 * {@link copyPlan}).
 */
export type AnnotationTool = "redact" | "highlight";

export interface Annotation extends Rect {
  tool: AnnotationTool;
}

/**
 * A drag is two points in whatever order the user's hand moved. Dragging up
 * and to the left produces a smaller end point than start point, and a rect
 * with a negative width fills nothing on a canvas — which would leave a
 * redaction the user believes they made and that isn't there.
 */
export function normalizeRect(from: Point, to: Point): Rect {
  return {
    x: Math.min(from.x, to.x),
    y: Math.min(from.y, to.y),
    width: Math.abs(to.x - from.x),
    height: Math.abs(to.y - from.y),
  };
}

/** Keeps a rect inside the bitmap, so a drag off the edge still fills to it. */
export function clampRect(rect: Rect, image: Size): Rect {
  const x = Math.max(0, Math.min(rect.x, image.width));
  const y = Math.max(0, Math.min(rect.y, image.height));
  return {
    x,
    y,
    width: Math.max(0, Math.min(rect.width, image.width - x)),
    height: Math.max(0, Math.min(rect.height, image.height - y)),
  };
}

/** Normalize then clamp — the whole drag-to-rect conversion in one call. */
export function rectFromDrag(from: Point, to: Point, image: Size): Rect {
  return clampRect(normalizeRect(from, to), image);
}

/**
 * A click without a drag is a click, not a one-pixel redaction. Below this the
 * shape is dropped rather than added to the stack, so the undo button doesn't
 * fill up with invisible entries.
 */
export function isDrawableRect(rect: Rect, minimum = 4): boolean {
  return rect.width >= minimum && rect.height >= minimum;
}

/**
 * The canvas is displayed scaled down (a 2560px screenshot in an 800px
 * dialog), but the shapes are stored in bitmap pixels — the export must be at
 * full resolution or the redaction is drawn at the wrong place on the image
 * that actually leaves the app.
 */
export function pointerToImage(offset: Point, box: Size, image: Size): Point {
  // A zero-sized box means the element hasn't been laid out; refusing to
  // divide by it is better than an Infinity that lands the rect nowhere.
  const scaleX = box.width > 0 ? image.width / box.width : 1;
  const scaleY = box.height > 0 ? image.height / box.height : 1;
  return {
    x: Math.max(0, Math.min(image.width, offset.x * scaleX)),
    y: Math.max(0, Math.min(image.height, offset.y * scaleY)),
  };
}

/**
 * Outline weight in bitmap pixels. A 2px stroke on a 4K screenshot shown at a
 * third of its size is invisible, and an invisible redaction border is exactly
 * the false confidence this feature exists to avoid.
 */
export function strokeWidth(imageWidth: number): number {
  if (!Number.isFinite(imageWidth) || imageWidth <= 0) return 2;
  return Math.max(2, Math.round(imageWidth / 480));
}

/* -------------------------------------------------------------------------
 * The system-context block
 * ---------------------------------------------------------------------- */

export interface SystemFacts {
  /** From `useAppInfo`. Null while it is still loading. */
  appVersion: string | null;
  userAgent: string;
  /** Dimensions of the capture, if one was taken. */
  shot: Size | null;
}

/**
 * Windows reports `Windows NT 10.0` from both Windows 10 and Windows 11, so
 * the block names the family and stops. A confidently wrong OS version in a
 * bug report is worse than no version at all.
 */
export function osName(userAgent: string): string {
  const ua = userAgent.toLowerCase();
  if (ua.includes("windows")) return "Windows";
  if (ua.includes("mac os x") || ua.includes("macintosh")) return "macOS";
  if (ua.includes("linux") || ua.includes("x11")) return "Linux";
  return "Unknown";
}

/**
 * What DevOS knows about itself and is willing to put in a public issue.
 *
 * Deliberately absent: `AppInfo.dbPath`, which is an absolute path containing
 * the user's account name, and terminal output of any kind — the terminal ring
 * buffer is where an exported `API_KEY=` lands, and no amount of "it's usually
 * fine" makes that a safe default. A line is omitted entirely when its fact is
 * unknown rather than filled with "unknown", so nothing here is invented.
 */
export function systemContextBlock(facts: SystemFacts): string {
  const lines = [
    "---",
    "_System context collected by DevOS — edit or delete any of it before filing._",
    "",
  ];
  if (facts.appVersion !== null && facts.appVersion.trim() !== "") {
    lines.push(`- DevOS: ${facts.appVersion.trim()}`);
  }
  lines.push(`- OS: ${osName(facts.userAgent)}`);
  if (facts.shot) {
    lines.push(`- Screenshot: ${facts.shot.width} × ${facts.shot.height}`);
  }
  return lines.join("\n");
}

/**
 * The body the compose step starts with: two blank lines for the user's own
 * words, then the block. The leading blank lines are stripped again by
 * {@link finalBody} before the review step, so an untouched draft doesn't post
 * with a gap at the top.
 */
export function initialIssueBody(facts: SystemFacts): string {
  return `\n\n${systemContextBlock(facts)}`;
}

/**
 * Tidies the draft *once*, on the way into the review step — never on the way
 * out of it. What the review pane shows is what gets posted, character for
 * character; a body silently reshaped after the user approved it would make
 * the approval meaningless.
 */
export function finalBody(draft: string): string {
  return draft.trim();
}

/** Titles are one line; a pasted newline would otherwise break the field. */
export function normalizeTitle(title: string): string {
  return title.replace(/\s+/g, " ").trim();
}

/* -------------------------------------------------------------------------
 * Repository targets
 * ---------------------------------------------------------------------- */

export interface TargetChoice {
  /** Case-folded `owner/name`; the identity used for selection and dedup. */
  key: string;
  owner: string;
  name: string;
  /** `owner/name` in the casing git reported. */
  label: string;
  /** Every remote that resolved to this repository, in the order seen. */
  remotes: string[];
}

/**
 * One row per repository, not per remote. A clone with `origin` and `push`
 * pointing at the same repo, or an SSH and an HTTPS remote for it, is one
 * choice — offering it twice implies a decision that doesn't exist. GitHub
 * treats owner and repo names case-insensitively, so `Owner/Repo` and
 * `owner/repo` fold together; the first spelling seen is the one displayed.
 *
 * Entries missing an owner or a name are dropped rather than rendered as
 * `/repo` — a target that cannot be filed against is not a choice.
 */
export function targetChoices(targets: IssueTarget[]): TargetChoice[] {
  const byKey = new Map<string, TargetChoice>();
  for (const target of targets) {
    const owner = target.owner.trim();
    const name = target.name.trim();
    if (owner === "" || name === "") continue;

    const key = `${owner.toLowerCase()}/${name.toLowerCase()}`;
    const remote = target.remote.trim();
    const existing = byKey.get(key);
    if (existing) {
      if (remote !== "" && !existing.remotes.includes(remote)) {
        existing.remotes.push(remote);
      }
      continue;
    }
    byKey.set(key, {
      key,
      owner,
      name,
      label: `${owner}/${name}`,
      remotes: remote === "" ? [] : [remote],
    });
  }
  return [...byKey.values()];
}

/** "origin", "origin, upstream", or "" when git named no remote. */
export function remotesLabel(choice: TargetChoice): string {
  return choice.remotes.join(", ");
}

/**
 * `origin` wins when it is there, because that is the repository a clone
 * belongs to; a fork's `upstream` is somebody else's issue tracker and filing
 * into it by default would be the wrong kind of surprise. With no `origin` at
 * all there is no basis to prefer one, so the first is preselected and the
 * list stays visible for the user to change.
 */
export function defaultTargetKey(choices: TargetChoice[]): string | null {
  const origin = choices.find((choice) => choice.remotes.includes("origin"));
  return origin?.key ?? choices[0]?.key ?? null;
}

/* -------------------------------------------------------------------------
 * Failure classification
 * ---------------------------------------------------------------------- */

/**
 * Four outcomes that look identical in a stringly-typed error and need four
 * different things from the user:
 *
 *   unconfigured — no `github_token` stored at all. Go and add one.
 *   auth         — one is stored and GitHub refused it. Replace it.
 *   notFound     — GitHub answered 404. Almost never "the repo is gone": the
 *                  REST API returns 404 rather than 403 for a private
 *                  repository the token cannot see, precisely so it doesn't
 *                  confirm the repository exists. The fix is the token's
 *                  repository access, not the URL.
 *   other        — show the message and don't pretend to know.
 *
 * Status matching is anchored to the `(HTTP nnn)` the backend formats, never a
 * bare number: GitHub error bodies are JSON that routinely carry a
 * `documentation_url` with digits in it, and `#http-404` inside an unrelated
 * failure's body must not be read as this call's status.
 */
export type IssueFailure = "unconfigured" | "auth" | "notFound" | "other";

export function classifyIssueError(error: unknown): IssueFailure {
  const text = String(error).toLowerCase();

  // Checked before the auth patterns on purpose. GitHub spends 403 on rate
  // limits as freely as on permissions, and sending someone to replace a
  // perfectly good token because they filed three issues in a minute costs
  // them a credential rotation and fixes nothing.
  if (text.includes("rate limit") || text.includes("secondary rate")) {
    return "other";
  }

  if (
    text.includes("rejected the token") ||
    text.includes("bad credentials") ||
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

  // "not found" on its own is far too common — a DNS failure says it, so does
  // a missing binary — so only the formatted status and an explicit mention of
  // the repository count.
  if (/\(http 404\)/.test(text) || text.includes("repository not found")) {
    return "notFound";
  }

  return "other";
}

/* -------------------------------------------------------------------------
 * Getting the image onto the clipboard
 * ---------------------------------------------------------------------- */

/**
 * `canvas` — write the flattened PNG from the webview. The bytes include the
 *   opaque redaction rectangles because they were painted into the bitmap.
 * `file`   — ask the backend to put the captured file on the clipboard. Only
 *   reachable when nothing was drawn, in which case the file and the flattened
 *   canvas are the same picture.
 * `refuse` — the webview cannot write an image and the shot was redacted.
 *   Copying the file on disk would hand over everything the user just covered
 *   up, so the button says why instead of quietly doing it.
 */
export type CopyPlan = "canvas" | "file" | "refuse";

export function copyPlan(options: {
  annotated: boolean;
  webviewClipboard: boolean;
}): CopyPlan {
  if (options.webviewClipboard) return "canvas";
  return options.annotated ? "refuse" : "file";
}
