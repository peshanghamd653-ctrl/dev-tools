/**
 * Test double for `@/shared/ipc/client`.
 *
 * Two things make the real module awkward to drive from a test:
 *
 * 1. Whether the app is inside the Tauri shell is a global that every page and
 *    every hook branches on. `isDesktopShell()` is a function evaluated per
 *    call, so the double just returns mutable state and one test file can
 *    drive both the desktop and browser paths. (It used to be an import-time
 *    const, which forced this double to expose a *getter* — a plain property
 *    was read once when the mock factory ran and went stale. That const is
 *    gone; the function form is what makes this straightforward.)
 * 2. `ipc` has ~70 methods. Listing them by hand would drift, so the double is
 *    a proxy that mints a `vi.fn()` on first access and remembers it. Tests
 *    still get full type checking, because the test file imports `ipc` from
 *    the real module path and TypeScript resolves the real signatures.
 *
 * Use it from a test file like this:
 *
 * ```ts
 * vi.mock("@/shared/ipc/client", async () => {
 *   const { createClientMock } = await import("@/shared/test/ipc");
 *   return createClientMock();
 * });
 * ```
 */
import { vi, type Mock } from "vitest";

type Client = typeof import("@/shared/ipc/client");

let desktopShell = true;
const stubs = new Map<string, Mock>();

/**
 * An unstubbed call is a test bug, not a silent no-op — reject loudly and name
 * the method so the failure points at what was forgotten.
 */
function defaultImplementation(name: string) {
  return () =>
    Promise.reject(new Error(`ipc.${name}() was called with no stub set`));
}

function stub(name: string): Mock {
  let existing = stubs.get(name);
  if (!existing) {
    existing = vi.fn(defaultImplementation(name));
    stubs.set(name, existing);
  }
  return existing;
}

/** Drive the desktop-shell branch that every page short-circuits on. */
export function setDesktopShell(value: boolean): void {
  desktopShell = value;
}

/**
 * Back to the defaults every test starts from: inside the desktop shell, with
 * no IPC method stubbed. Call from `beforeEach`.
 */
export function resetClientMock(): void {
  desktopShell = true;
  for (const [name, fn] of stubs) {
    fn.mockReset();
    fn.mockImplementation(defaultImplementation(name));
  }
}

export function createClientMock() {
  const ipc = new Proxy({} as Client["ipc"], {
    get: (_target, property) =>
      typeof property === "string" ? stub(property) : undefined,
  });

  return {
    isDesktopShell: () => desktopShell,
    ipc,
    onKernelEvent: vi.fn(() => () => {}),
  };
}
