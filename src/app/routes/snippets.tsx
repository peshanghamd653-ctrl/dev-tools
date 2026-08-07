import { createFileRoute } from "@tanstack/react-router";

import { SnippetsPage } from "@/features/snippets/SnippetsPage";

export const Route = createFileRoute("/snippets")({
  component: SnippetsPage,
});
