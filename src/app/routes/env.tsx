import { createFileRoute } from "@tanstack/react-router";

import { EnvFilePage } from "@/features/envfile/EnvFilePage";

export const Route = createFileRoute("/env")({
  component: EnvFilePage,
});
