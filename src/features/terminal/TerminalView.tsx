import { useEffect, useRef } from "react";

import { ipc } from "@/shared/ipc/client";
import { useResolvedTheme } from "@/shared/theme/useTheme";
import { applyThemeToAll, getInstance } from "./registry";

/**
 * Mounts a persistent xterm instance into this pane. The instance and its
 * scrollback live in the registry; this component only attaches/detaches it.
 */
export function TerminalView({ sessionId }: { sessionId: string }) {
  const wrapperRef = useRef<HTMLDivElement>(null);
  const theme = useResolvedTheme();

  // Re-colour on mount as well as on change: an instance created while a
  // different theme was active is still cached, and this is where it gets
  // caught up. Runs after the class swap, so the tokens read here are the
  // new ones.
  useEffect(() => {
    applyThemeToAll();
  }, [theme]);

  useEffect(() => {
    const wrapper = wrapperRef.current;
    const instance = getInstance(sessionId);
    if (!wrapper || !instance) return;

    wrapper.appendChild(instance.container);
    if (!instance.opened) {
      instance.term.open(instance.container);
      instance.opened = true;
    }

    let frame = 0;
    const fitAndReport = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        instance.fit.fit();
        const { cols, rows } = instance.term;
        void ipc.termResize(sessionId, cols, rows).catch(() => {
          /* session gone */
        });
      });
    };

    fitAndReport();
    instance.term.focus();

    const observer = new ResizeObserver(fitAndReport);
    observer.observe(wrapper);

    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      if (instance.container.parentNode === wrapper) {
        wrapper.removeChild(instance.container);
      }
    };
  }, [sessionId]);

  return (
    <div
      ref={wrapperRef}
      className="h-full w-full overflow-hidden bg-background p-2"
    />
  );
}
