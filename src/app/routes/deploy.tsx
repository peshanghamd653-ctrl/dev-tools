import { createFileRoute } from "@tanstack/react-router";

import { DeployPage } from "@/features/deploy/DeployPage";

export const Route = createFileRoute("/deploy")({
  component: DeployPage,
});
