import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Code2,
  Globe,
  RotateCw,
} from "lucide-react";
import { toast } from "sonner";

import { isDesktopShell, ipc, onBrowserNav } from "@/shared/ipc/client";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { useBrowserStore } from "./store";
import { normalizeUrl } from "./utils";

/**
 * An embedded browser pane, not an iframe: a real child webview
 * (`tauri::Window::add_child`) that this page positions over a placeholder
 * `<div>` it owns. The placeholder never shows the actual page — it exists
 * so this component has a DOM element to measure and keep the native
 * webview's bounds in sync with, via `ResizeObserver`.
 *
 * "DevTools" is the real WebView2 inspector (`Webview::open_devtools`), not
 * a hand-rolled console panel — see the module doc comment on
 * `browser_commands.rs` for why.
 */
export function BrowserPage() {
  const currentUrl = useBrowserStore((s) => s.currentUrl);
  const setCurrentUrl = useBrowserStore((s) => s.setCurrentUrl);
  const [input, setInput] = useState(currentUrl ?? "");
  const placeholderRef = useRef<HTMLDivElement>(null);
  const openedRef = useRef(false);

  async function openUrl(raw: string) {
    const url = normalizeUrl(raw);
    const rect = placeholderRef.current?.getBoundingClientRect();
    if (!rect) return;
    try {
      await ipc.browserOpen(url, rect.x, rect.y, rect.width, rect.height);
      openedRef.current = true;
      setCurrentUrl(url);
      setInput(url);
    } catch (error) {
      toast.error(String(error));
    }
  }

  // Re-show (not re-create) whatever was open on a previous visit, then keep
  // the native webview's bounds glued to the placeholder — window resizes,
  // the sidebar collapsing, anything that moves this element. Hides on
  // unmount rather than closing: navigating back to this page should not
  // mean losing scroll position and re-fetching everything.
  useEffect(() => {
    if (!isDesktopShell()) return;
    if (currentUrl) void openUrl(currentUrl);

    const unlistenNav = onBrowserNav((url) => {
      setCurrentUrl(url);
      setInput(url);
    });

    const el = placeholderRef.current;
    const observer = el
      ? new ResizeObserver(() => {
          if (!openedRef.current || !el) return;
          const rect = el.getBoundingClientRect();
          void ipc
            .browserSetBounds(rect.x, rect.y, rect.width, rect.height)
            .catch(() => {
              // The pane may have been hidden by a fast navigate-away
              // between the observer firing and this call landing — not
              // worth surfacing to the user.
            });
        })
      : null;
    if (el && observer) observer.observe(el);

    return () => {
      unlistenNav();
      observer?.disconnect();
      if (openedRef.current) void ipc.browserHide().catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once per mount; openUrl closes over state intentionally read fresh via refs/store getters, not re-subscribed on every currentUrl change.
  }, []);

  if (!isDesktopShell()) {
    return (
      <div className="mx-auto max-w-md py-12 text-center">
        <p className="font-medium">The browser pane needs the desktop shell</p>
        <p className="text-sm text-muted-foreground">
          A browser tab has nothing to embed a second browser inside.
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-9 shrink-0 items-center gap-1 border-b px-2">
        <Button
          variant="ghost"
          size="icon"
          className="size-7"
          aria-label="Back"
          onClick={() => void ipc.browserBack().catch(() => {})}
        >
          <ArrowLeft className="size-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="size-7"
          aria-label="Forward"
          onClick={() => void ipc.browserForward().catch(() => {})}
        >
          <ArrowRight className="size-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="size-7"
          aria-label="Reload"
          onClick={() => void ipc.browserReload().catch(() => {})}
        >
          <RotateCw className="size-3.5" />
        </Button>
        <form
          className="flex-1"
          onSubmit={(e) => {
            e.preventDefault();
            void (openedRef.current
              ? ipc
                  .browserNavigate(normalizeUrl(input))
                  .then(() => setCurrentUrl(normalizeUrl(input)))
                  .catch((error: unknown) => toast.error(String(error)))
              : openUrl(input));
          }}
        >
          <Input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="localhost:3000, or any URL"
            className="h-7 font-mono text-xs"
            aria-label="Address"
          />
        </form>
        <Button
          variant="ghost"
          size="sm"
          className="h-7 gap-1.5"
          disabled={!openedRef.current}
          onClick={() => void ipc.browserOpenDevtools().catch(() => {})}
        >
          <Code2 className="size-3.5" />
          DevTools
        </Button>
      </div>

      <div ref={placeholderRef} className="relative min-h-0 flex-1">
        {!currentUrl && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-muted-foreground">
            <Globe className="size-6" />
            <p className="text-sm">
              Enter a URL above — your dev server, or any site
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
