import { Check, Monitor } from "lucide-react";

import { cn } from "@/shared/lib/utils";
import { themeClass, THEMES, type ThemeId } from "./themes";
import { useResolvedTheme, useThemePreference } from "./useTheme";

/**
 * Theme picker for Settings → Appearance.
 *
 * Each option previews itself by rendering a miniature shell *inside* the
 * theme's own class. Because the tokens are plain custom properties they
 * re-cascade into the subtree, so the swatch is the real palette rather than a
 * second, drift-prone copy of it.
 *
 * Native radios do the accessibility work: one tab stop, arrow keys move
 * between options, the checked state is announced.
 */
export function ThemePicker() {
  const [preference, setPreference] = useThemePreference();
  const resolved = useResolvedTheme();

  return (
    <fieldset>
      <legend className="sr-only">Theme</legend>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <ThemeOption
          value="system"
          label="System"
          description="Follows your OS appearance setting."
          checked={preference === "system"}
          onSelect={() => setPreference("system")}
          preview={<SystemPreview />}
          badge={`Now: ${THEMES.find((t) => t.id === resolved)?.label}`}
        />

        {THEMES.map((theme) => (
          <ThemeOption
            key={theme.id}
            value={theme.id}
            label={theme.label}
            description={theme.description}
            checked={preference === theme.id}
            onSelect={() => setPreference(theme.id)}
            preview={<ShellPreview theme={theme.id} />}
          />
        ))}
      </div>
    </fieldset>
  );
}

function ThemeOption({
  value,
  label,
  description,
  checked,
  onSelect,
  preview,
  badge,
}: {
  value: string;
  label: string;
  description: string;
  checked: boolean;
  onSelect: () => void;
  preview: React.ReactNode;
  badge?: string;
}) {
  return (
    <label
      className={cn(
        "group relative cursor-pointer rounded-lg border p-1.5 transition-colors",
        "has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-ring",
        checked
          ? "border-primary/60 bg-primary/5"
          : "border-border hover:border-input hover:bg-accent/50",
      )}
      title={description}
    >
      <input
        type="radio"
        name="devos-theme"
        value={value}
        checked={checked}
        onChange={onSelect}
        className="sr-only"
      />

      {preview}

      <span className="mt-1.5 flex items-center gap-1 px-0.5">
        <span className="truncate text-xs font-medium">{label}</span>
        {checked && (
          <Check aria-hidden className="ml-auto size-3.5 shrink-0 text-primary" />
        )}
      </span>
      <span className="block truncate px-0.5 text-[10px] text-muted-foreground">
        {badge ?? description}
      </span>
    </label>
  );
}

/** A miniature DevOS shell painted with `theme`'s own tokens. */
function ShellPreview({
  theme,
  className,
}: {
  theme: ThemeId;
  className?: string;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        "flex h-16 overflow-hidden rounded-md border border-border bg-background",
        themeClass(theme),
        className,
      )}
    >
      <span className="flex w-1/4 flex-col gap-1 border-r border-sidebar-border bg-sidebar p-1.5">
        <span className="h-1 w-full rounded-full bg-sidebar-primary" />
        <span className="h-1 w-3/4 rounded-full bg-muted-foreground/70" />
        <span className="h-1 w-2/3 rounded-full bg-muted-foreground/40" />
      </span>

      <span className="flex min-w-0 flex-1 flex-col gap-1 p-1.5">
        <span className="h-1.5 w-1/2 rounded-full bg-foreground/80" />
        <span className="flex flex-1 flex-col justify-center gap-1 rounded-sm border border-border bg-card px-1.5">
          <span className="h-1 w-full rounded-full bg-muted-foreground/50" />
          <span className="flex gap-1">
            <span className="h-1.5 w-5 rounded-full bg-primary" />
            <span className="h-1.5 w-3 rounded-full bg-destructive" />
            <span className="h-1.5 w-2 rounded-full bg-emerald-500" />
          </span>
        </span>
      </span>
    </span>
  );
}

/** Both ends of the system setting, split diagonally. */
function SystemPreview() {
  return (
    <span aria-hidden className="relative block h-16">
      <ShellPreview theme="daylight" className="absolute inset-0" />
      <ShellPreview
        theme="midnight"
        className="absolute inset-0 [clip-path:polygon(100%_0,100%_100%,0_100%)]"
      />
      <span className="absolute inset-0 flex items-center justify-center">
        <Monitor className="size-4 text-foreground/70 drop-shadow-[0_1px_2px_rgb(0_0_0/0.5)]" />
      </span>
    </span>
  );
}
