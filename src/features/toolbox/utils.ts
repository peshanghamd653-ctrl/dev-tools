/**
 * Pure logic for the utility toolbox. Kept separate from the components so
 * it's testable without rendering anything — every tool here is a small,
 * synchronous (or Web Crypto-backed) text transform, not a reason to reach
 * for a library.
 */

export type ToolResult =
  { ok: true; result: string } | { ok: false; error: string };

export function formatJson(input: string, indent = 2): ToolResult {
  try {
    return {
      ok: true,
      result: JSON.stringify(JSON.parse(input), null, indent),
    };
  } catch (error) {
    return { ok: false, error: jsonErrorMessage(error) };
  }
}

export function minifyJson(input: string): ToolResult {
  try {
    return { ok: true, result: JSON.stringify(JSON.parse(input)) };
  } catch (error) {
    return { ok: false, error: jsonErrorMessage(error) };
  }
}

function jsonErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Invalid JSON";
}

/** UTF-8 safe — plain `btoa` throws on any character past Latin-1. */
export function base64Encode(input: string): string {
  const bytes = new TextEncoder().encode(input);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function base64Decode(input: string): ToolResult {
  try {
    const binary = atob(input);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    return {
      ok: true,
      result: new TextDecoder("utf-8", { fatal: true }).decode(bytes),
    };
  } catch {
    return {
      ok: false,
      error: "Not valid base64 (or not valid UTF-8 once decoded)",
    };
  }
}

export function urlEncode(input: string): string {
  return encodeURIComponent(input);
}

export function urlDecode(input: string): ToolResult {
  try {
    return { ok: true, result: decodeURIComponent(input) };
  } catch {
    return { ok: false, error: "Not a validly percent-encoded string" };
  }
}

export interface DecodedJwt {
  header: string;
  payload: string;
}

/**
 * Decodes a JWT's header and payload — never verifies the signature, and
 * never could: that needs the issuer's key, which this tool doesn't have and
 * has no way to obtain. This is for reading a token you already have, not
 * for trusting one.
 */
export function decodeJwt(
  token: string,
): { ok: true; value: DecodedJwt } | { ok: false; error: string } {
  const parts = token.trim().split(".");
  if (parts.length !== 3) {
    return {
      ok: false,
      error: "A JWT has three dot-separated parts; this has " + parts.length,
    };
  }
  try {
    const header = JSON.stringify(
      JSON.parse(base64UrlDecode(parts[0]!)),
      null,
      2,
    );
    const payload = JSON.stringify(
      JSON.parse(base64UrlDecode(parts[1]!)),
      null,
      2,
    );
    return { ok: true, value: { header, payload } };
  } catch {
    return {
      ok: false,
      error: "Header or payload is not base64url-encoded JSON",
    };
  }
}

function base64UrlDecode(segment: string): string {
  const padded = segment.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded + "=".repeat((4 - (padded.length % 4)) % 4));
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

export type HashAlgorithm = "SHA-1" | "SHA-256" | "SHA-512";

export async function hashText(
  algorithm: HashAlgorithm,
  input: string,
): Promise<string> {
  const bytes = new TextEncoder().encode(input);
  const digest = await crypto.subtle.digest(algorithm, bytes);
  return [...new Uint8Array(digest)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

export interface RegexMatch {
  match: string;
  index: number;
  groups: string[];
}

export function testRegex(
  pattern: string,
  flags: string,
  text: string,
): { ok: true; matches: RegexMatch[] } | { ok: false; error: string } {
  let regex: RegExp;
  try {
    // `g` is required for `matchAll`; adding it if the user didn't is a
    // convenience, not a behavior change they'd notice — a pattern with no
    // `g` still only ever finds one match either way.
    regex = new RegExp(pattern, flags.includes("g") ? flags : flags + "g");
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Invalid pattern",
    };
  }
  const matches: RegexMatch[] = [];
  for (const m of text.matchAll(regex)) {
    matches.push({
      match: m[0],
      index: m.index,
      groups: m.slice(1).map((g) => g ?? ""),
    });
  }
  return { ok: true, matches };
}
