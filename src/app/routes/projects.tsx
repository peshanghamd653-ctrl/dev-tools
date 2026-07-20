import { createFileRoute } from "@tanstack/react-router";

import { ProjectsPage } from "@/features/projects/ProjectsPage";

export const Route = createFileRoute("/projects")({
  component: ProjectsPage,
});
