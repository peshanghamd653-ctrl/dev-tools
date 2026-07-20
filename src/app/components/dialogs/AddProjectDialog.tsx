import { zodResolver } from "@hookform/resolvers/zod";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";

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
            <Label htmlFor="project-name">Name</Label>
            <Input
              id="project-name"
              placeholder="e.g. devos"
              autoFocus
              {...form.register("name")}
            />
            {form.formState.errors.name && (
              <p className="text-xs text-destructive">
                {form.formState.errors.name.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="project-path">Folder path</Label>
            <Input
              id="project-path"
              placeholder="C:\code\my-project"
              className="font-mono text-xs"
              {...form.register("path")}
            />
            {form.formState.errors.path && (
              <p className="text-xs text-destructive">
                {form.formState.errors.path.message}
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
