/**
 * Rendering helper for component tests. Every page in `src/features` reads its
 * data through TanStack Query, so nothing renders without a client in context.
 */
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { configure, render, type RenderResult } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";

/**
 * These pages render a lot of nodes, and `findByRole` rebuilds the
 * accessibility tree on every poll. On a cold Windows worker a single render
 * can outlast Testing Library's 1s default, which shows up as a "unable to
 * find" failure that has nothing to do with the component. Five seconds is
 * still short enough that a genuinely missing element fails quickly.
 */
configure({ asyncUtilTimeout: 5000 });

/**
 * Retries and background refetches are the two things that turn a component
 * test into a flaky one: a retry hides the error path under a timeout, and a
 * refetch fires after the assertion has already run. Both are off here, which
 * also means a rejected query reaches the UI on the first tick.
 */
export function createTestQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
        staleTime: Infinity,
        refetchOnMount: false,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
      },
      mutations: { retry: false },
    },
  });
}

export function renderWithClient(
  ui: ReactElement,
  client: QueryClient = createTestQueryClient(),
): RenderResult & { client: QueryClient } {
  function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  }

  // Object.assign rather than a spread: RenderResult's bound query helpers do
  // not survive being spread into a fresh object literal.
  return Object.assign(render(ui, { wrapper: Wrapper }), { client });
}
