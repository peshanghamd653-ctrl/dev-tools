import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const notificationKeys = {
  list: ["notifications", "list"] as const,
  unread: ["notifications", "unread"] as const,
};

export function useNotifications() {
  return useQuery({
    queryKey: notificationKeys.list,
    queryFn: ipc.notificationsList,
    enabled: isDesktopShell(),
  });
}

export function useUnreadCount() {
  return useQuery({
    queryKey: notificationKeys.unread,
    queryFn: ipc.notificationsUnreadCount,
    enabled: isDesktopShell(),
  });
}

export function useNotificationActions() {
  const queryClient = useQueryClient();
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["notifications"] });

  return {
    markRead: useMutation({
      mutationFn: (id: string) => ipc.notificationMarkRead(id),
      onSettled: invalidate,
    }),
    markAllRead: useMutation({
      mutationFn: () => ipc.notificationsMarkAllRead(),
      onSettled: invalidate,
    }),
  };
}
