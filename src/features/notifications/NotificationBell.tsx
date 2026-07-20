import { Bell, CheckCheck } from "lucide-react";

import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import {
  useNotificationActions,
  useNotifications,
  useUnreadCount,
} from "./hooks";

const LEVEL_DOTS: Record<string, string> = {
  error: "bg-red-400",
  warning: "bg-yellow-400",
  info: "bg-primary",
};

export function NotificationBell() {
  const { data: notifications } = useNotifications();
  const { data: unread } = useUnreadCount();
  const { markRead, markAllRead } = useNotificationActions();

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="relative size-8 text-muted-foreground"
          aria-label={
            unread ? `Notifications (${unread} unread)` : "Notifications"
          }
        >
          <Bell className="size-4" />
          {Boolean(unread) && (
            <span className="absolute right-1 top-1 flex size-3.5 items-center justify-center rounded-full bg-primary text-[9px] font-semibold text-primary-foreground">
              {unread! > 9 ? "9+" : unread}
            </span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-96 p-0">
        <div className="flex items-center justify-between border-b px-3 py-2">
          <p className="text-sm font-medium">Notifications</p>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 gap-1 px-2 text-xs text-muted-foreground"
            disabled={!unread}
            onClick={() => markAllRead.mutate()}
          >
            <CheckCheck className="size-3.5" />
            Mark all read
          </Button>
        </div>
        <div className="max-h-96 overflow-y-auto">
          {notifications?.length === 0 && (
            <p className="px-3 py-8 text-center text-sm text-muted-foreground">
              Nothing yet — job results and agent reports land here.
            </p>
          )}
          {notifications?.map((n) => (
            <button
              key={n.id}
              type="button"
              className={cn(
                "flex w-full items-start gap-2.5 border-b border-border/50 px-3 py-2 text-left last:border-b-0 hover:bg-accent/60",
                n.read && "opacity-60",
              )}
              onClick={() => {
                if (!n.read) markRead.mutate(n.id);
              }}
            >
              <span
                className={cn(
                  "mt-1.5 size-2 shrink-0 rounded-full",
                  n.read
                    ? "bg-muted-foreground/30"
                    : (LEVEL_DOTS[n.level] ?? "bg-primary"),
                )}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm">{n.title}</span>
                {n.body && (
                  <span className="line-clamp-2 block text-xs text-muted-foreground">
                    {n.body}
                  </span>
                )}
                <span className="block pt-0.5 text-[10px] uppercase tracking-wide text-muted-foreground/70">
                  {n.module} · {timeAgo(n.createdAt)}
                </span>
              </span>
            </button>
          ))}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function timeAgo(ms: number): string {
  const seconds = Math.floor((Date.now() - ms) / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}
