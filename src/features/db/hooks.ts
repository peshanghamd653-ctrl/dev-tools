import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { ipc, isDesktopShell } from "@/shared/ipc/client";

export const dbKeys = {
  connections: ["db", "connections"] as const,
  schema: (id: string) => ["db", "schema", id] as const,
};

export function useDbConnections() {
  return useQuery({
    queryKey: dbKeys.connections,
    queryFn: ipc.dbConnections,
    enabled: isDesktopShell(),
  });
}

export function useDbSchema(id: string | null) {
  return useQuery({
    queryKey: dbKeys.schema(id ?? "none"),
    queryFn: () => ipc.dbSchema(id ?? ""),
    enabled: isDesktopShell() && Boolean(id),
    staleTime: 30_000,
  });
}

export function useDbConnect() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ name, path }: { name: string; path: string }) =>
      ipc.dbConnect(name, path),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: dbKeys.connections }),
  });
}

export function useDbConnectionDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.dbConnectionDelete(id),
    onSuccess: (_data, id) => {
      queryClient.removeQueries({ queryKey: dbKeys.schema(id) });
      return queryClient.invalidateQueries({ queryKey: dbKeys.connections });
    },
  });
}

/**
 * Running SQL is a mutation, not a query: it's an explicit user action with
 * side effects, so it must never be cached, retried or auto-refetched. A
 * statement that actually wrote invalidates the schema, whose row counts and
 * table list are now stale.
 */
export function useDbQuery() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      sql,
      allowWrite,
    }: {
      id: string;
      sql: string;
      allowWrite: boolean;
    }) => ipc.dbQuery(id, sql, allowWrite),
    onSuccess: (result, { id }) => {
      if (!result.readOnly) {
        return queryClient.invalidateQueries({ queryKey: dbKeys.schema(id) });
      }
    },
  });
}

export function useDbTableRows() {
  return useMutation({
    mutationFn: ({
      id,
      table,
      limit,
    }: {
      id: string;
      table: string;
      limit: number;
    }) => ipc.dbTableRows(id, table, limit),
  });
}
