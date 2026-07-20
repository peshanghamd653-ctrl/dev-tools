import { createFileRoute } from "@tanstack/react-router";

import { TerminalPage } from "@/features/terminal/TerminalPage";

export const Route = createFileRoute("/terminal")({
  component: TerminalPage,
});
