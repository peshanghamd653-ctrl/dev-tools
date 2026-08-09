import { lazy, Suspense, useEffect, useState, type ReactNode } from "react";
import { Toaster } from "sonner";

import { Sidebar } from "@/app/components/Sidebar";
import { Topbar } from "@/app/components/Topbar";
import { useGlobalHotkeys } from "@/shared/hooks/useGlobalHotkeys";
import { useKernelEventBridge } from "@/shared/hooks/useKernelEvents";
import { useDialogStore } from "@/shared/stores/dialogs";
import { useUiStore } from "@/shared/stores/ui";
import { toasterTheme } from "@/shared/theme/apply";
import { useThemeSync } from "@/shared/theme/useTheme";
import { TooltipProvider } from "@/shared/ui/tooltip";

// The command palette and the three global dialogs are all closed on boot and
// render nothing until opened — but statically importing them dragged their
// whole dependency tail into the entry chunk: zod + react-hook-form +
// @hookform/resolvers (the two form dialogs), cmdk (the palette), and the
// screenshot annotator. That is ~113 kB of minified JS the webview parsed on
// every launch to display nothing. See docs/performance.md.
const CommandPalette = lazy(() =>
  import("@/app/components/CommandPalette").then((m) => ({
    default: m.CommandPalette,
  })),
);
const SymbolSearchDialog = lazy(() =>
  import("@/app/components/SymbolSearchDialog").then((m) => ({
    default: m.SymbolSearchDialog,
  })),
);
const CreateWorkspaceDialog = lazy(() =>
  import("@/app/components/dialogs/CreateWorkspaceDialog").then((m) => ({
    default: m.CreateWorkspaceDialog,
  })),
);
const AddProjectDialog = lazy(() =>
  import("@/app/components/dialogs/AddProjectDialog").then((m) => ({
    default: m.AddProjectDialog,
  })),
);
const CaptureIssueDialog = lazy(() =>
  import("@/features/issues/CaptureIssueDialog").then((m) => ({
    default: m.CaptureIssueDialog,
  })),
);

/**
 * True once the browser has gone idle after first paint, and true forever
 * after. Deliberately not a `setTimeout(0)`: the point is to yield until the
 * shell has actually painted, not merely until the current task ends.
 */
function useIdleAfterPaint() {
  const [idled, setIdled] = useState(false);

  useEffect(() => {
    if (idled) return;
    // WebKit — the webview on macOS and Linux — only grew requestIdleCallback
    // recently, and Tauri targets older WebKitGTK than that.
    if (typeof window.requestIdleCallback !== "function") {
      const timer = setTimeout(() => setIdled(true), 200);
      return () => clearTimeout(timer);
    }
    const handle = window.requestIdleCallback(() => setIdled(true), {
      timeout: 2000,
    });
    return () => window.cancelIdleCallback(handle);
  }, [idled]);

  return idled;
}

export function AppShell({ children }: { children: ReactNode }) {
  useKernelEventBridge();
  useGlobalHotkeys();
  // Keeps <html> on the right theme class after boot: preference changes and
  // live OS appearance flips. First paint is handled by index.html's script.
  const theme = useThemeSync();

  // Overlays mount one idle tick after first paint — closed, exactly as they
  // were before — so from then on every open animates and behaves identically
  // to a static import. The `!idled &&` guards are what keep that true without
  // costing anything: they pin both selectors to a constant `false` once the
  // idle mount has happened, so toggling the palette never re-renders the
  // shell. Before that they are live, so a keypress in the first few hundred
  // milliseconds still opens the overlay rather than being swallowed.
  const idled = useIdleAfterPaint();
  const paletteWanted = useUiStore((s) => !idled && s.paletteOpen);
  const symbolSearchWanted = useUiStore((s) => !idled && s.symbolSearchOpen);
  const dialogWanted = useDialogStore(
    (s) =>
      !idled &&
      (s.createWorkspaceOpen || s.addProjectOpen || s.captureIssueOpen),
  );
  const overlaysMounted =
    idled || paletteWanted || symbolSearchWanted || dialogWanted;

  return (
    <TooltipProvider delayDuration={200}>
      <div className="flex h-screen w-screen overflow-hidden">
        <Sidebar />
        <div className="flex min-w-0 flex-1 flex-col">
          <Topbar />
          <main className="min-w-0 flex-1 overflow-y-auto">{children}</main>
        </div>
      </div>
      {/* `null` is the honest fallback: every one of these renders nothing at
          all while closed, so there is no pane to leave blank and nothing to
          lay out around. */}
      {overlaysMounted && (
        <Suspense fallback={null}>
          <CommandPalette />
          <SymbolSearchDialog />
          <CreateWorkspaceDialog />
          <AddProjectDialog />
          <CaptureIssueDialog />
        </Suspense>
      )}
      <Toaster theme={toasterTheme(theme)} position="bottom-right" richColors />
    </TooltipProvider>
  );
}
