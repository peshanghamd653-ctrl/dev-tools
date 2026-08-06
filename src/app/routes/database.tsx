import { createFileRoute } from "@tanstack/react-router";

import { DatabasePage } from "@/features/db/DatabasePage";

export const Route = createFileRoute("/database")({
  component: DatabasePage,
});
