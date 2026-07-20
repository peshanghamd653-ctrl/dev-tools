import { createFileRoute } from "@tanstack/react-router";

import { GitPage } from "@/features/git/GitPage";

export const Route = createFileRoute("/git")({
  component: GitPage,
});
