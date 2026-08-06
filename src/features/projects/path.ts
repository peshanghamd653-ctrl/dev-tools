/**
 * The final segment of a filesystem path, used to pre-fill a project name from
 * the folder the user picked.
 *
 * Accepts either separator: the picker returns `\` on Windows, but a pasted
 * path may use `/`, and WSL-style paths do too. Trailing separators are
 * stripped so `C:\code\devos\` still yields `devos`.
 *
 * Returns an empty string when there is no meaningful name — a drive root, a
 * bare `/`, or whitespace. The caller leaves the name field untouched in that
 * case rather than filling it with `C:` or an empty string.
 */
export function folderNameFromPath(path: string): string {
  const trimmed = path.trim().replace(/[\\/]+$/, "");
  if (!trimmed) return "";

  const segment = trimmed.split(/[\\/]/).pop() ?? "";
  // `C:` is a drive designator, not a folder anyone would name a project.
  if (/^[A-Za-z]:$/.test(segment)) return "";

  return segment;
}
