import { zodResolver } from "@hookform/resolvers/zod";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";

import { useCreateWorkspace } from "@/features/workspaces/hooks";
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
  name: z
    .string()
    .trim()
    .min(1, "Name is required")
    .max(40, "Keep it under 40 characters"),
});

type FormValues = z.infer<typeof schema>;

export function CreateWorkspaceDialog() {
  const open = useDialogStore((s) => s.createWorkspaceOpen);
  const setOpen = useDialogStore((s) => s.setCreateWorkspaceOpen);
  const createWorkspace = useCreateWorkspace();

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { name: "" },
  });

  function onSubmit(values: FormValues) {
    createWorkspace.mutate(values.name, {
      onSuccess: (workspace) => {
        toast.success(`Workspace "${workspace.name}" created`);
        form.reset();
        setOpen(false);
      },
      onError: (error) => toast.error(String(error)),
    });
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>Create workspace</DialogTitle>
          <DialogDescription>
            Workspaces group related projects and keep their own state.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="workspace-name">Name</Label>
            <Input
              id="workspace-name"
              placeholder="e.g. Client Work"
              autoFocus
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
            <Button type="submit" disabled={createWorkspace.isPending}>
              Create
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
