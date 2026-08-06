import { zodResolver } from "@hookform/resolvers/zod";
// Aliased: this component already binds `open` to the dialog's own visibility.
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";

import { folderNameFromPath } from "@/features/projects/path";
import { useAddProject } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { useDialogStore } from "@/shared/stores/dialogs";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";

const schema = z.object({
  name: z.string().trim().min(1, "Name is required").max(60),
  path: z.string().trim().min(2, "Enter the folder path of the project"),
});

type FormValues = z.infer<typeof schema>;

export function AddProjectDialog() {
  const open = useDialogStore((s) => s.addProjectOpen);
  const setOpen = useDialogStore((s) => s.setAddProjectOpen);
  const activeWorkspace = useActiveWorkspace();
  const addProject = useAddProject(activeWorkspace?.id);

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { name: "", path: "" },
  });

  async function browseForFolder() {
    try {
      const picked = await openFolderPicker({
        directory: true,
        multiple: false,
        title: "Select a project folder",
      });
      // `null` means the user cancelled — not an error, and not worth a toast.
      if (typeof picked !== "string") return;

      form.setValue("path", picked, { shouldValidate: true });

      // Pre-fill the name from the folder, but never overwrite something the
      // user has already typed.
      if (!form.getValues("name").trim()) {
        const derived = folderNameFromPath(picked);
        if (derived) form.setValue("name", derived, { shouldValidate: true });
      }
    } catch (error) {
      // The picker only exists inside the desktop shell; in a browser tab this
      // throws rather than opening anything.
      toast.error(`Could not open the folder picker: ${error}`);
    }
  }

  function onSubmit(values: FormValues) {
    addProject.mutate(values, {
      onSuccess: (project) => {
        toast.success(`Project "${project.name}" added`);
        form.reset();
        setOpen(false);
      },
      onError: (error) => toast.error(String(error)),
    });
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add project</DialogTitle>
          <DialogDescription>
            Register an existing folder as a project in{" "}
            <span className="text-foreground">
              {activeWorkspace?.name ?? "the active workspace"}
            </span>
            .
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="project-path">Folder</Label>
            <div className="flex gap-2">
              <Input
                id="project-path"
                placeholder="C:\code\my-project"
                className="font-mono text-xs"
                {...form.register("path")}
              />
              <Button
                type="button"
                variant="outline"
                className="shrink-0"
                onClick={() => void browseForFolder()}
              >
                <FolderOpen className="size-4" />
                Browse…
              </Button>
            </div>
            {form.formState.errors.path && (
              <p className="text-xs text-destructive">
                {form.formState.errors.path.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="project-name">Name</Label>
            <Input
              id="project-name"
              placeholder="e.g. devos"
              {...form.register("name")}
            />
            {form.formState.errors.name && (
              <p className="text-xs text-destructive">
                {form.formState.errors.name.message}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => setOpen(false)}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={addProject.isPending}>
              Add project
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
