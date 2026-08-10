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

/**
 * `DOMParser`/`XMLSerializer` are Web APIs, not a dependency — pretty-
 * printing is the only part that needs hand-written logic, since
 * `XMLSerializer` doesn't indent.
 */
export function formatXml(input: string, indentSize = 2): ToolResult {
  const trimmed = input.trim();
  if (!trimmed) return { ok: false, error: "Nothing to format" };
  const doc = new DOMParser().parseFromString(trimmed, "application/xml");
  const parseError = doc.querySelector("parsererror");
  if (parseError) {
    return {
      ok: false,
      error: parseError.textContent?.trim() || "Invalid XML",
    };
  }
  if (!doc.documentElement) {
    return { ok: false, error: "No root element" };
  }
  return {
    ok: true,
    result: indentXmlElement(doc.documentElement, 0, " ".repeat(indentSize)),
  };
}

function indentXmlElement(el: Element, depth: number, unit: string): string {
  const pad = unit.repeat(depth);
  const attrs = Array.from(el.attributes)
    .map((a) => ` ${a.name}="${a.value}"`)
    .join("");
  const children = Array.from(el.childNodes).filter(
    (n) => n.nodeType !== Node.TEXT_NODE || Boolean(n.textContent?.trim()),
  );
  if (children.length === 0) {
    return `${pad}<${el.tagName}${attrs} />`;
  }
  if (children.length === 1 && children[0]!.nodeType === Node.TEXT_NODE) {
    return `${pad}<${el.tagName}${attrs}>${children[0]!.textContent!.trim()}</${el.tagName}>`;
  }
  const inner = children
    .filter((n): n is Element => n.nodeType === Node.ELEMENT_NODE)
    .map((child) => indentXmlElement(child, depth + 1, unit))
    .join("\n");
  return `${pad}<${el.tagName}${attrs}>\n${inner}\n${pad}</${el.tagName}>`;
}

export interface DiffLine {
  type: "same" | "added" | "removed";
  text: string;
}

/**
 * Line-based diff (classic LCS via dynamic programming) — works equally for
 * two pastes of formatted JSON or any other text, so it covers both
 * "JSON diff" and "text diff" without a structural JSON differ.
 */
export function diffLines(a: string, b: string): DiffLine[] {
  const left = a.split("\n");
  const right = b.split("\n");
  const n = left.length;
  const m = right.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () =>
    new Array<number>(m + 1).fill(0),
  );
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i]![j] =
        left[i] === right[j]
          ? lcs[i + 1]![j + 1]! + 1
          : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
    }
  }
  const result: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (left[i] === right[j]) {
      result.push({ type: "same", text: left[i]! });
      i++;
      j++;
    } else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) {
      result.push({ type: "removed", text: left[i]! });
      i++;
    } else {
      result.push({ type: "added", text: right[j]! });
      j++;
    }
  }
  while (i < n) {
    result.push({ type: "removed", text: left[i]! });
    i++;
  }
  while (j < m) {
    result.push({ type: "added", text: right[j]! });
    j++;
  }
  return result;
}

/**
 * One direction only — JSON to YAML. Parsing arbitrary YAML back to JSON
 * would mean hand-rolling block scalars, anchors, and flow collections to
 * do it correctly, which is exactly the kind of thing this module's own
 * rule says not to do without a library. Emitting YAML for a value that's
 * already valid JSON needs none of that.
 */
export function jsonToYaml(input: string): ToolResult {
  try {
    const value: unknown = JSON.parse(input);
    return { ok: true, result: yamlValue(value, 0) };
  } catch (error) {
    return { ok: false, error: jsonErrorMessage(error) };
  }
}

function yamlValue(value: unknown, depth: number): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean" || typeof value === "number") {
    return String(value);
  }
  if (typeof value === "string") return yamlScalarString(value);
  if (Array.isArray(value)) return yamlArray(value, depth);
  return yamlObject(value as Record<string, unknown>, depth);
}

function yamlScalarString(s: string): string {
  const needsQuoting =
    s === "" ||
    /^\s|\s$/.test(s) ||
    /^(true|false|null|~|-?\d+(\.\d+)?)$/i.test(s) ||
    /[:#{}[\],&*!|>'"%@`\n]/.test(s);
  return needsQuoting ? JSON.stringify(s) : s;
}

function isNonEmptyContainer(value: unknown): value is object {
  return (
    value !== null && typeof value === "object" && Object.keys(value).length > 0
  );
}

function yamlArray(arr: unknown[], depth: number): string {
  if (arr.length === 0) return "[]";
  const pad = "  ".repeat(depth);
  const childPad = "  ".repeat(depth + 1);
  return arr
    .map((item) => {
      if (isNonEmptyContainer(item)) {
        // The nested block is rendered as if it were a normal value at
        // depth+1, then the dash absorbs its first line's own indent —
        // "- " is exactly one indent level wide, so what's left lines up
        // with the rest of the block underneath it.
        const [first, ...rest] = yamlValue(item, depth + 1).split("\n");
        const firstLine = first!.startsWith(childPad)
          ? first!.slice(childPad.length)
          : first!;
        return [`${pad}- ${firstLine}`, ...rest].join("\n");
      }
      return `${pad}- ${yamlValue(item, depth)}`;
    })
    .join("\n");
}

function yamlObject(obj: Record<string, unknown>, depth: number): string {
  const keys = Object.keys(obj);
  if (keys.length === 0) return "{}";
  const pad = "  ".repeat(depth);
  return keys
    .map((key) => {
      const value = obj[key];
      const keyStr = yamlScalarString(key);
      if (isNonEmptyContainer(value)) {
        return `${pad}${keyStr}:\n${yamlValue(value, depth + 1)}`;
      }
      return `${pad}${keyStr}: ${yamlValue(value, depth)}`;
    })
    .join("\n");
}

export interface HttpStatus {
  code: number;
  text: string;
  description: string;
}

export const HTTP_STATUS_CODES: HttpStatus[] = [
  {
    code: 100,
    text: "Continue",
    description:
      "The initial part of a request has been received and the client should continue.",
  },
  {
    code: 101,
    text: "Switching Protocols",
    description:
      "The server is switching protocols as requested by the client.",
  },
  { code: 200, text: "OK", description: "The request succeeded." },
  {
    code: 201,
    text: "Created",
    description: "The request succeeded and a new resource was created.",
  },
  {
    code: 202,
    text: "Accepted",
    description:
      "The request was accepted for processing, but is not complete.",
  },
  {
    code: 204,
    text: "No Content",
    description: "The request succeeded; there is no content to send.",
  },
  {
    code: 206,
    text: "Partial Content",
    description: "Only part of the resource was sent, per a Range header.",
  },
  {
    code: 301,
    text: "Moved Permanently",
    description: "The resource has a new permanent URI.",
  },
  {
    code: 302,
    text: "Found",
    description: "The resource is temporarily at a different URI.",
  },
  {
    code: 303,
    text: "See Other",
    description: "The response can be found at another URI via GET.",
  },
  {
    code: 304,
    text: "Not Modified",
    description: "The cached response is still valid.",
  },
  {
    code: 307,
    text: "Temporary Redirect",
    description: "Like 302, but the method must not change.",
  },
  {
    code: 308,
    text: "Permanent Redirect",
    description: "Like 301, but the method must not change.",
  },
  {
    code: 400,
    text: "Bad Request",
    description: "The server could not understand the request.",
  },
  {
    code: 401,
    text: "Unauthorized",
    description:
      "Authentication is required and has failed or not been provided.",
  },
  {
    code: 403,
    text: "Forbidden",
    description:
      "The server understood the request but refuses to authorize it.",
  },
  {
    code: 404,
    text: "Not Found",
    description: "The requested resource could not be found.",
  },
  {
    code: 405,
    text: "Method Not Allowed",
    description: "The method is not supported for this resource.",
  },
  {
    code: 406,
    text: "Not Acceptable",
    description: "No content matching the Accept headers was found.",
  },
  {
    code: 408,
    text: "Request Timeout",
    description: "The server timed out waiting for the request.",
  },
  {
    code: 409,
    text: "Conflict",
    description:
      "The request conflicts with the current state of the resource.",
  },
  {
    code: 410,
    text: "Gone",
    description: "The resource is no longer available and won't be again.",
  },
  {
    code: 411,
    text: "Length Required",
    description: "A Content-Length header is required.",
  },
  {
    code: 413,
    text: "Payload Too Large",
    description: "The request body is larger than the server will accept.",
  },
  {
    code: 414,
    text: "URI Too Long",
    description: "The request URI is longer than the server will interpret.",
  },
  {
    code: 415,
    text: "Unsupported Media Type",
    description: "The request body's media type is not supported.",
  },
  {
    code: 418,
    text: "I'm a Teapot",
    description: "The server refuses to brew coffee with a teapot (RFC 2324).",
  },
  {
    code: 422,
    text: "Unprocessable Entity",
    description: "The request was well-formed but semantically invalid.",
  },
  {
    code: 425,
    text: "Too Early",
    description:
      "The server is unwilling to risk processing a replayed request.",
  },
  {
    code: 429,
    text: "Too Many Requests",
    description: "The client has sent too many requests in a given time.",
  },
  {
    code: 431,
    text: "Request Header Fields Too Large",
    description: "The request's headers are too large.",
  },
  {
    code: 451,
    text: "Unavailable For Legal Reasons",
    description: "The resource is withheld for legal reasons.",
  },
  {
    code: 500,
    text: "Internal Server Error",
    description: "The server encountered an unexpected condition.",
  },
  {
    code: 501,
    text: "Not Implemented",
    description: "The server does not support the functionality required.",
  },
  {
    code: 502,
    text: "Bad Gateway",
    description: "The server, acting as a gateway, got an invalid response.",
  },
  {
    code: 503,
    text: "Service Unavailable",
    description: "The server is not ready to handle the request.",
  },
  {
    code: 504,
    text: "Gateway Timeout",
    description:
      "The server, acting as a gateway, did not get a response in time.",
  },
  {
    code: 505,
    text: "HTTP Version Not Supported",
    description: "The server does not support the HTTP version used.",
  },
];

export function lookupHttpStatus(query: string): HttpStatus[] {
  const q = query.trim().toLowerCase();
  if (!q) return HTTP_STATUS_CODES;
  return HTTP_STATUS_CODES.filter(
    (s) => String(s.code).includes(q) || s.text.toLowerCase().includes(q),
  );
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
