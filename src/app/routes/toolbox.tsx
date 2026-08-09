import { createFileRoute } from "@tanstack/react-router";

import { ToolboxPage } from "@/features/toolbox/ToolboxPage";

export const Route = createFileRoute("/toolbox")({
  component: ToolboxPage,
});
