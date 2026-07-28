import { createFileRoute } from "@tanstack/react-router";

import { DockerPage } from "@/features/docker/DockerPage";

export const Route = createFileRoute("/docker")({
  component: DockerPage,
});
