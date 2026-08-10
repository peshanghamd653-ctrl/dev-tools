import { useEffect } from "react";
import { useNavigate } from "@tanstack/react-router";

import { useDialogStore } from "@/shared/stores/dialogs";
import { useUiStore } from "@/shared/stores/ui";

/**
 * App-wide keyboard shortcuts. Registered once in the shell.
 *   Ctrl+K  command palette      Ctrl+B  toggle sidebar
 *   Ctrl+T  go to symbol         Ctrl+1  dashboard
 *   Ctrl+2  projects
 *   Ctrl+Shift+D  deployments    Ctrl+Shift+N  snippets
 *   Ctrl+Shift+M  MCP servers    Ctrl+Shift+S  report a bug
 *   Ctrl+Shift+U  toolbox        Ctrl+Shift+E  security
 *   Ctrl+Shift+V  env variables  Ctrl+Shift+B  browser
 *   Ctrl+,  settings
 */
export function useGlobalHotkeys() {
  const navigate = useNavigate();
  const togglePalette = useUiStore((s) => s.togglePalette);
  const toggleSymbolSearch = useUiStore((s) => s.toggleSymbolSearch);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const setCaptureIssueOpen = useDialogStore((s) => s.setCaptureIssueOpen);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.metaKey)) return;

      // Shift-modified bindings are matched on the physical key and an
      // explicit `shiftKey` check, never on `e.key`: with Shift held the
      // browser reports "D", but so does a plain Ctrl+D with Caps Lock on —
      // keying off "D" alone would fire this on Ctrl+D. Returning here also
      // leaves the unmodified bindings below exactly as they were, since
      // Shift already changed `e.key` ("K", "!", "<") past every case.
      if (e.shiftKey) {
        if (e.code === "KeyD") {
          e.preventDefault();
          void navigate({ to: "/deploy" });
        }
        if (e.code === "KeyN") {
          // Ctrl+Shift+N is the browser's "new incognito window" in a tab;
          // inside the shell there is no such thing to shadow.
          e.preventDefault();
          void navigate({ to: "/snippets" });
        }
        if (e.code === "KeyS") {
          // Opens the dialog, which takes the screenshot before rendering
          // anything — pressing this must not put a dialog in the picture.
          e.preventDefault();
          setCaptureIssueOpen(true);
        }
        if (e.code === "KeyM") {
          e.preventDefault();
          void navigate({ to: "/mcp" });
        }
        if (e.code === "KeyU") {
          e.preventDefault();
          void navigate({ to: "/toolbox" });
        }
        if (e.code === "KeyE") {
          e.preventDefault();
          void navigate({ to: "/security" });
        }
        if (e.code === "KeyV") {
          // Ctrl+Shift+V is "paste without formatting" in a browser; inside
          // the shell there is no rich-text field for that to apply to.
          e.preventDefault();
          void navigate({ to: "/env" });
        }
        if (e.code === "KeyB") {
          // Ctrl+B (unmodified, below) already toggles the sidebar; Shift
          // distinguishes this from that rather than colliding with it.
          e.preventDefault();
          void navigate({ to: "/browser" });
        }
        return;
      }

      switch (e.key) {
        case "k":
          e.preventDefault();
          togglePalette();
          break;
        case "t":
          e.preventDefault();
          toggleSymbolSearch();
          break;
        case "b":
          e.preventDefault();
          toggleSidebar();
          break;
        case "1":
          e.preventDefault();
          void navigate({ to: "/" });
          break;
        case "2":
          e.preventDefault();
          void navigate({ to: "/projects" });
          break;
        case "3":
          e.preventDefault();
          void navigate({ to: "/terminal" });
          break;
        case "4":
          e.preventDefault();
          void navigate({ to: "/git" });
          break;
        case "5":
          e.preventDefault();
          void navigate({ to: "/ai" });
          break;
        case "6":
          e.preventDefault();
          void navigate({ to: "/files" });
          break;
        case "7":
          e.preventDefault();
          void navigate({ to: "/docker" });
          break;
        case "8":
          e.preventDefault();
          void navigate({ to: "/api" });
          break;
        case "9":
          e.preventDefault();
          void navigate({ to: "/database" });
          break;
        case "0":
          e.preventDefault();
          void navigate({ to: "/monitors" });
          break;
        case ",":
          e.preventDefault();
          void navigate({ to: "/settings" });
          break;
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    navigate,
    setCaptureIssueOpen,
    togglePalette,
    toggleSymbolSearch,
    toggleSidebar,
  ]);
}
