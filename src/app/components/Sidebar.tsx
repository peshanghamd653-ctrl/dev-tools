import { Link } from "@tanstack/react-router";
import { PanelLeftClose, PanelLeftOpen } from "lucide-react";

import { primaryNav, upcomingNav } from "@/app/nav";
import { cn } from "@/shared/lib/utils";
import { useUiStore } from "@/shared/stores/ui";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Separator } from "@/shared/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

export function Sidebar() {
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);

  return (
    <aside
      className={cn(
        "flex h-full shrink-0 flex-col border-r border-sidebar-border bg-sidebar transition-[width] duration-200",
        collapsed ? "w-14" : "w-60",
      )}
    >
      <div
        className={cn(
          "flex h-12 items-center gap-2 px-3",
          collapsed && "justify-center px-0",
        )}
      >
        <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary font-semibold text-primary-foreground text-xs">
          D
        </div>
        {!collapsed && (
          <span className="font-semibold text-sm tracking-tight">DevOS</span>
        )}
      </div>

      <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 py-2">
        {primaryNav.map((item) => (
          <Tooltip key={item.to} disableHoverableContent>
            <TooltipTrigger asChild>
              <Link
                to={item.to}
                className={cn(
                  "flex h-8 items-center gap-2.5 rounded-md px-2 text-sm text-sidebar-foreground/70",
                  "hover:bg-sidebar-accent hover:text-sidebar-foreground",
                  collapsed && "justify-center px-0",
                )}
                activeProps={{
                  className: "bg-sidebar-accent text-sidebar-foreground",
                }}
                activeOptions={{ exact: item.to === "/" }}
              >
                <item.icon className="size-4 shrink-0" />
                {!collapsed && <span className="truncate">{item.label}</span>}
                {!collapsed && item.shortcut && (
                  <kbd className="ml-auto text-[10px] text-muted-foreground tracking-wide">
                    {item.shortcut}
                  </kbd>
                )}
              </Link>
            </TooltipTrigger>
            {collapsed && (
              <TooltipContent side="right">{item.label}</TooltipContent>
            )}
          </Tooltip>
        ))}

        {upcomingNav.length > 0 && (
          <>
            <Separator className="my-2" />

            {!collapsed && (
              <p className="px-2 pb-1 text-[10px] font-medium text-muted-foreground uppercase tracking-wider">
                Coming soon
              </p>
            )}
            {upcomingNav.map((item) => (
              <Tooltip key={item.label} disableHoverableContent>
                <TooltipTrigger asChild>
                  <div
                    aria-disabled
                    className={cn(
                      "flex h-8 cursor-default items-center gap-2.5 rounded-md px-2 text-sm text-sidebar-foreground/35",
                      collapsed && "justify-center px-0",
                    )}
                  >
                    <item.icon className="size-4 shrink-0" />
                    {!collapsed && (
                      <span className="truncate">{item.label}</span>
                    )}
                    {!collapsed && (
                      <Badge
                        variant="outline"
                        className="ml-auto h-4 px-1 text-[9px] text-muted-foreground"
                      >
                        {item.milestone}
                      </Badge>
                    )}
                  </div>
                </TooltipTrigger>
                <TooltipContent side="right">
                  {item.label} — planned for {item.milestone}
                </TooltipContent>
              </Tooltip>
            ))}
          </>
        )}
      </nav>

      <div className={cn("p-2", collapsed && "flex justify-center")}>
        <Button
          variant="ghost"
          size="icon"
          className="size-8 text-muted-foreground"
          onClick={toggleSidebar}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        >
          {collapsed ? (
            <PanelLeftOpen className="size-4" />
          ) : (
            <PanelLeftClose className="size-4" />
          )}
        </Button>
      </div>
    </aside>
  );
}
