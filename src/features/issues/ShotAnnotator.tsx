import { useEffect, useState, type RefObject } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { ImageOff } from "lucide-react";

import type { CapturedShot } from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import {
  isDrawableRect,
  pointerToImage,
  rectFromDrag,
  strokeWidth,
  type Annotation,
  type AnnotationTool,
  type Point,
} from "./compose";

/**
 * Redaction is solid black, not blurred and not pixelated. Both of those read
 * as "obscured" while being visibly reconstructible, which invites exactly the
 * false confidence that gets an API key into a public issue. This is a hole in
 * the picture and looks like one.
 *
 * Canvas takes colour strings, not Tailwind classes, so these are the one
 * place in the feature with literal colours; they track the design system's
 * red = destructive / yellow = attention convention.
 */
const REDACT_FILL = "#000000";
const REDACT_EDGE = "#f43f5e";
const HIGHLIGHT_EDGE = "#facc15";

/**
 * One canvas, no overlay. Every shape is painted into the same bitmap the
 * export reads back, so there is no layer that could be dropped on the way out
 * and no state in which the user sees a redaction the exported PNG does not
 * have. The in-progress drag is repainted from the shape stack each frame for
 * the same reason — nothing is composited outside the bitmap.
 */
export function ShotAnnotator({
  shot,
  tool,
  annotations,
  canvasRef,
  onCommit,
  onReady,
  onLoadError,
}: {
  shot: CapturedShot;
  tool: AnnotationTool;
  annotations: Annotation[];
  canvasRef: RefObject<HTMLCanvasElement | null>;
  onCommit: (annotation: Annotation) => void;
  /**
   * True once the bitmap is on the canvas. Until then the canvas is at its
   * default 300×150 and reading it back would hand the next step a blank
   * image, so the parent gates "Continue" on this.
   *
   * This and `onLoadError` must be stable references — `useState` setters,
   * not inline closures. Both are dependencies of the load effect, and a new
   * function every render would restart the image fetch forever.
   */
  onReady: (ready: boolean) => void;
  onLoadError: (message: string | null) => void;
}) {
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const [failed, setFailed] = useState(false);
  const [drag, setDrag] = useState<{ from: Point; to: Point } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setImage(null);
    setFailed(false);
    onReady(false);
    onLoadError(null);

    const element = new Image();
    // Without this the bitmap taints the canvas and `toBlob` throws at the
    // very end of the flow, after the user has done all the annotation work.
    // Requesting CORS up front turns that into a failure on load, which is the
    // one place it can still be explained and recovered from.
    element.crossOrigin = "anonymous";
    element.onload = () => {
      if (cancelled) return;
      setImage(element);
      onReady(true);
    };
    element.onerror = () => {
      if (cancelled) return;
      setFailed(true);
      onLoadError(
        "DevOS could not read the screenshot it just captured. The file is at " +
          `${shot.path} — without it there is nothing to annotate or attach.`,
      );
    };
    element.src = convertFileSrc(shot.path);

    return () => {
      cancelled = true;
      element.onload = null;
      element.onerror = null;
    };
  }, [onLoadError, onReady, shot.path]);

  const width = image?.naturalWidth ?? 0;
  const height = image?.naturalHeight ?? 0;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !image) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(image, 0, 0);
    const edge = strokeWidth(canvas.width);
    for (const annotation of annotations) paint(ctx, annotation, edge);
    if (drag) {
      const rect = rectFromDrag(drag.from, drag.to, { width, height });
      if (isDrawableRect(rect)) paint(ctx, { tool, ...rect }, edge);
    }
  }, [annotations, canvasRef, drag, height, image, tool, width]);

  function pointOf(event: React.PointerEvent<HTMLCanvasElement>): Point {
    const box = event.currentTarget.getBoundingClientRect();
    return pointerToImage(
      { x: event.clientX - box.left, y: event.clientY - box.top },
      { width: box.width, height: box.height },
      { width, height },
    );
  }

  if (failed) {
    return (
      <div className="flex min-h-40 flex-col items-center justify-center gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-6 text-center">
        <ImageOff className="size-5 text-red-300" />
        <p className="text-sm text-red-300">The capture could not be opened.</p>
      </div>
    );
  }

  return (
    <div className="flex min-h-40 items-center justify-center overflow-hidden rounded-md border bg-black/40">
      {!image && (
        <p className="p-6 text-sm text-muted-foreground">Loading capture…</p>
      )}
      <canvas
        ref={canvasRef}
        width={width || undefined}
        height={height || undefined}
        aria-label="Screenshot — drag to draw a rectangle"
        className={cn(
          "max-h-[52vh] w-auto max-w-full touch-none select-none",
          image ? "block" : "hidden",
          tool === "redact" ? "cursor-crosshair" : "cursor-copy",
        )}
        onPointerDown={(event) => {
          if (!image || event.button !== 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          const point = pointOf(event);
          setDrag({ from: point, to: point });
        }}
        onPointerMove={(event) => {
          if (!drag) return;
          setDrag({ from: drag.from, to: pointOf(event) });
        }}
        onPointerUp={(event) => {
          if (!drag) return;
          const rect = rectFromDrag(drag.from, pointOf(event), {
            width,
            height,
          });
          setDrag(null);
          // A click that never moved is a click, not a one-pixel redaction.
          if (isDrawableRect(rect)) onCommit({ tool, ...rect });
        }}
        onPointerCancel={() => setDrag(null)}
      />
    </div>
  );
}

function paint(
  ctx: CanvasRenderingContext2D,
  annotation: Annotation,
  edge: number,
) {
  const { x, y, width, height } = annotation;
  if (annotation.tool === "redact") {
    // Opaque, and painted into the bitmap — the pixels underneath are gone
    // from every copy of the image that leaves this dialog.
    ctx.fillStyle = REDACT_FILL;
    ctx.fillRect(x, y, width, height);
    ctx.strokeStyle = REDACT_EDGE;
  } else {
    ctx.strokeStyle = HIGHLIGHT_EDGE;
  }
  ctx.lineWidth = edge;
  ctx.strokeRect(
    x + edge / 2,
    y + edge / 2,
    Math.max(0, width - edge),
    Math.max(0, height - edge),
  );
}
