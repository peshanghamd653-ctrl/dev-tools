import { createFileRoute } from "@tanstack/react-router";

import { AiPage } from "@/features/ai/AiPage";

export const Route = createFileRoute("/ai")({
  component: AiPage,
});
