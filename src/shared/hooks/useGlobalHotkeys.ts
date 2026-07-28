import { useEffect } from "react";
import { useNavigate } from "@tanstack/react-router";

import { useUiStore } from "@/shared/stores/ui";

/**
 * App-wide keyboard shortcuts. Registered once in the shell.
 *   Ctrl+K  command palette      Ctrl+B  toggle sidebar
 *   Ctrl+1  dashboard            Ctrl+2  projects
 *   Ctrl+,  settings
 */
export function useGlobalHotkeys() {
  const navigate = useNavigate();
  const togglePalette = useUiStore((s) => s.togglePalette);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.metaKey)) return;
      switch (e.key) {
        case "k":
          e.preventDefault();
          togglePalette();
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
        case ",":
          e.preventDefault();
          void navigate({ to: "/settings" });
          break;
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate, togglePalette, toggleSidebar]);
}
