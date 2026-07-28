import { createFileRoute } from "@tanstack/react-router";

import { ApiClientPage } from "@/features/api/ApiClientPage";

export const Route = createFileRoute("/api")({
  component: ApiClientPage,
});
