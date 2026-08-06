/**
 * Minimal Node typings for the theme tests.
 *
 * `tokens.test.ts` has to read `globals.css` as text to prove every theme
 * assigns every token. The obvious route — `import css from "…css?raw"` —
 * does not work: vitest's CSS-disabling plugin rewrites *any* request whose
 * id matches `.css` (query included) to an empty module, so the import
 * silently yields `""` and the test would pass against nothing.
 *
 * So the test reads the file off disk instead. `@types/node` is not a
 * dependency of this app and pulling it in would put Node's globals in scope
 * for every frontend file, which is exactly what we don't want. Declaring the
 * two functions actually used keeps that blast radius at zero. These merge
 * harmlessly if `@types/node` is ever added.
 */

declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
}

declare module "node:process" {
  /** vitest runs with the project root as cwd. */
  export function cwd(): string;
}
