/**
 * Names are the only part of a secret this app can ever see. `secret_list`
 * returns names and nothing else by design (see docs/security.md), so the
 * name is doing all the work of telling the user what a row holds — worth
 * the few rules below.
 */

/** Nothing clever: the store takes the name verbatim, minus stray whitespace. */
export function normalizeSecretName(raw: string): string {
  return raw.trim();
}

export type NameStatus = "empty" | "new" | "overwrite";

/**
 * Saving an existing name replaces its value silently — the list can't show
 * that, because it has no values to compare. Telling the user before they
 * press Save is the only warning available.
 */
export function secretNameStatus(
  existing: string[],
  draft: string,
): NameStatus {
  const name = normalizeSecretName(draft);
  if (name === "") return "empty";
  return existing.includes(name) ? "overwrite" : "new";
}

/**
 * Alphabetical, case-insensitively, so `Vercel_token` and `anthropic-api-key`
 * don't sort into two separate blocks by capitalisation.
 */
export function sortSecretNames(names: string[]): string[] {
  return [...names].sort((a, b) =>
    a.localeCompare(b, undefined, { sensitivity: "base" }),
  );
}
