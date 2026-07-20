import { useMemo } from "react";

import { cn } from "@/shared/lib/utils";
import { parseUnifiedDiff } from "./diff";

export function DiffViewer({ diff }: { diff: string }) {
  const lines = useMemo(() => parseUnifiedDiff(diff), [diff]);

  if (diff.trim() === "") {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        No changes in this file.
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto font-mono text-xs leading-5">
      <table className="w-full border-collapse">
        <tbody>
          {lines.map((line, i) => {
            if (line.kind === "meta") return null;
            if (line.kind === "hunk") {
              return (
                <tr key={i} className="bg-primary/10 text-primary">
                  <td colSpan={3} className="select-none px-3 py-0.5">
                    {line.text}
                  </td>
                </tr>
              );
            }
            return (
              <tr
                key={i}
                className={cn(
                  line.kind === "add" && "bg-emerald-500/10",
                  line.kind === "del" && "bg-red-500/10",
                )}
              >
                <td className="w-10 select-none pr-2 text-right text-muted-foreground/50">
                  {line.oldNo ?? ""}
                </td>
                <td className="w-10 select-none pr-2 text-right text-muted-foreground/50">
                  {line.newNo ?? ""}
                </td>
                <td
                  className={cn(
                    "whitespace-pre-wrap break-all pl-2",
                    line.kind === "add" && "text-emerald-300",
                    line.kind === "del" && "text-red-300",
                  )}
                >
                  <span className="select-none">
                    {line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}
                  </span>{" "}
                  {line.text}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
