import { createFileRoute } from "@tanstack/react-router";

import { FileExplorerPage } from "@/features/files/FileExplorerPage";

export const Route = createFileRoute("/files")({
  component: FileExplorerPage,
});
