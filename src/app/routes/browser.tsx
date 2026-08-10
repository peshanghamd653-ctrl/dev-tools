import { createFileRoute } from "@tanstack/react-router";

import { BrowserPage } from "@/features/browser/BrowserPage";

export const Route = createFileRoute("/browser")({
  component: BrowserPage,
});
