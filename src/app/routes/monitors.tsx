import { createFileRoute } from "@tanstack/react-router";

import { MonitorsPage } from "@/features/monitors/MonitorsPage";

export const Route = createFileRoute("/monitors")({
  component: MonitorsPage,
});
