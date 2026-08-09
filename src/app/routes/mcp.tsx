import { createFileRoute } from "@tanstack/react-router";

import { McpPage } from "@/features/mcp/McpPage";

export const Route = createFileRoute("/mcp")({
  component: McpPage,
});
