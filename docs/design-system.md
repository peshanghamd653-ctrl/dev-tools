# Design System

Visual language: Linear/Raycast-grade dark UI. Dark-first is a default, not a
constraint: **Midnight** — the original palette — is what you get until you
choose otherwise, and it is byte-for-byte the palette DevOS shipped before the
theme system landed. See [Themes](#themes).

## Foundations

| Token | Value | Notes |
|---|---|---|
| Font (UI) | Inter Variable | `@fontsource-variable/inter` |
| Font (mono) | JetBrains Mono Variable | terminal, code, paths, diffs |
| Color space | `oklch()` | perceptually uniform, used for every token |
| Radius | `0.5rem` base | `--radius-sm/md/lg/xl` derived from it |
| Component style | shadcn/ui, `new-york` variant | see `components.json` |

Color tokens (`background`, `foreground`, `card`, `popover`, `primary`,
`secondary`, `muted`, `accent`, `destructive`, `border`, `input`, `ring`,
`sidebar-*`, `chart-1..5`) are exposed to Tailwind once in `globals.css` under
`@theme inline` and consumed as utilities (`bg-background`,
`text-muted-foreground`, etc.) — never hex literals in components.

## Themes

A theme is **a complete assignment of the token set above** — nothing more.
There are no per-theme component variables and no `.dark` variant switching
(the `dark:` utilities shadcn/ui ships stay dormant in every theme, which is
what keeps Midnight identical to the pre-theme app). Adding a theme means
adding one `.theme-*` block to `globals.css` and one entry to
`src/shared/theme/themes.ts`.

| Theme | Appearance | Character |
|---|---|---|
| **Midnight** (default) | dark | The original DevOS palette, unchanged. Cool near-black, indigo accent. |
| **Daylight** | light | Same neutral hue (285°), inverted. Every accent darkened to clear 4.5:1 on white. |
| **Obsidian** | dark | Near-black for OLED. Higher contrast throughout; bright indigo primary with a near-black label. |

Plus **System**, which follows `prefers-color-scheme` (→ Midnight / Daylight)
and tracks live changes to it while the app is open.

### How it is wired

```
index.html  <script> …reads localStorage, adds .theme-<id> to <html>… </script>   ← before first paint
src/shared/theme/themes.ts    registry + resolveTheme(preference, systemDark)     ← pure
src/shared/theme/apply.ts     class swap on <html>, prefers-color-scheme watcher
src/shared/stores/theme.ts    zustand persist, key `devos-theme`, {preference}
src/shared/theme/useTheme.ts  useThemeSync() — mounted once in AppShell
src/shared/theme/ThemePicker  Settings → Appearance
```

**No flash of the wrong theme.** The class is applied by an inline script in
`<head>`, before the stylesheet and before the app bundle. A `useEffect` runs
after first paint and would flash; `useThemeSync` therefore only handles
*changes* (a new pick, or the OS flipping appearance). The script also paints
`<html>` inline for the dev-server case, where the stylesheet is injected by
JS well after first paint; `applyThemeClass` strips that inline value on mount
so the cascade takes over. The boot script is the one place the theme list is
duplicated — `src/shared/theme/themes.test.ts` parses `index.html` and fails
if it drifts from the registry.

Preference persists under `devos-theme` and survives restart; an unrecognised
or corrupt stored value falls back to Midnight rather than leaving the app
unstyled. `:root` also carries the Midnight tokens for the same reason.

### The status palette layer

Semantic tokens are not the whole picture. Status that *carries meaning* —
monitor up/down, deployment state, git file status, the SQL write-toggle
warning — is painted in feature code with raw Tailwind palette utilities
(`text-emerald-300`, `bg-yellow-500/10`). Those shades were chosen to glow on
near-black; on white they disappear.

Tailwind v4 compiles them to `var(--color-emerald-300)`, so the palette is
itself themeable. `.theme-daylight` re-points shades 200–400 of the families
in use to the dark end of the same hue (preserving shade order) and nudges the
500s — which are fills, not text — far enough to clear 3:1 on white. Dark
themes keep Tailwind's defaults. No component edits, no new variables.

If you add a status colour, use an existing family; a lone new family will
read correctly in the dark themes and vanish in Daylight.
`src/shared/theme/tokens.test.ts` fails on a partially re-pointed family.

### Contrast

Measured in-browser (Chrome resolves `oklch()` and composites alpha in sRGB,
so these are painted values, not estimates). Every text pair in Daylight and
Obsidian meets WCAG AA (4.5:1); every status mark meets 3:1 for non-text.

| Pair | Midnight | Daylight | Obsidian |
|---|---|---|---|
| body text on page | 15.51 | 16.77 | 18.38 |
| body text on card | 14.50 | 17.50 | 17.63 |
| muted text on card | 5.71 | 6.82 | 8.65 |
| muted text on sidebar | 6.28 | 6.11 | 9.18 |
| `text-primary` link on card | 4.05 | 6.69 | 5.92 |
| primary button label | 4.24 | 6.51 | 6.02 |
| destructive button (white label) | 4.83 | 6.55 | 4.86 |
| `text-destructive` on card | 3.71 | 6.55 | 4.02 |
| monitor UP chip | 10.15 | 5.19 | 11.37 |
| monitor DOWN chip | 8.40 | 5.45 | 9.22 |
| monitor PAUSED chip | 12.77 | 4.98 | 14.41 |
| deploy BUILDING chip | 9.34 | 5.54 | 10.44 |
| SQL write-mode banner | 12.77 | 4.98 | 14.41 |
| git status letter (M / A / D / R) | 6.19–11.37 | 7.18–8.49 | 6.77–12.42 |
| monitor up dot (non-text, ≥3) | 7.23 | 5.71 | 7.90 |
| sparkline ok bar (non-text, ≥3) | 4.15 | 3.20 | 4.35 |

Midnight's four sub-4.5 rows are pre-existing and deliberately untouched —
changing them would shift every existing screen. Both new themes clear AA
there.

## Layout primitives (implemented)

```
┌──────────┬───────────────────────────────────────────────────────────────┐
│ ▣ DevOS  │ Personal ▾                        [ ⌘ Search or run…  Ctrl K ]│
│          ├───────────────────────────────────────────────────────────────┤
│ ▸ Dash   │                                                               │
│ ▸ Projects                     ROUTED CONTENT                            │
│ ▸ Terminal                                                               │
│ ▸ Git                                                                    │
│ ▸ AI Assistant                                                           │
│ ▸ Settings                                                               │
│ ──────── │                                                               │
│ COMING   │                                                               │
│ ░ Docker    M3                                                           │
│ ░ API Client M3                                                          │
│ ░ Database   M3                                                          │
│          │                                                               │
│ ⟨ collapse│                                                              │
└──────────┴───────────────────────────────────────────────────────────────┘
```

- Sidebar collapses to an icon rail (`Ctrl+B`). Planned modules render
  disabled with a milestone `Badge` — the roadmap is visible without faking
  a screen. The list is empty as of M4 and the section hides entirely when
  it is, rather than leaving a header over nothing.
- Topbar hosts the workspace switcher and the palette trigger (with its
  shortcut shown inline).

## Command palette

`cmdk`-based (`src/shared/ui/command.tsx`), grouped sections: Navigation,
Actions, Modules (populated from the kernel's `CommandRegistry`, so a new
module's contributed commands appear automatically). `Ctrl+K` opens it from
anywhere; `Esc` closes.

## Feature-page conventions

- **List + detail split panes** (Git, AI Assistant): fixed-width sidebar
  (`280–340px`) + flexible content area, `grid-cols-[Npx_1fr]`.
  Tab strips inside a pane use a small pill-button pattern (`h-6`, rounded,
  active state = `bg-accent`).
- **Empty states**: icon in a muted circle, one-line explanation, one
  primary CTA button. Never a blank page.
- **Status color convention**: emerald = success/live/staged-add, red =
  error/deleted, yellow = modified/warning, sky = rename/copy — used
  consistently across git file-status letters, terminal session dots, and
  tool-call activity rows. These are re-pointed per theme by the
  [status palette layer](#the-status-palette-layer); state is also carried by
  shape (filled dot, ringed dot, pause glyph, dashed ring) so it survives
  without colour.
- **Chips/badges for attached context**: the AI page's project-context and
  tools-grant chips use the same pattern — pill button, filled when active
  (`border-primary/40 bg-primary/10`), outline + muted when off.

## Motion

`motion` (Framer Motion successor) is installed but not yet used anywhere.
Reserve it for shell-level transitions (route change, palette open/close)
once the core flows are stable — don't add per-component animation ad hoc.

## Accessibility baseline

- Every interactive icon-only button has an `aria-label`.
- Focus rings are always visible (`:focus-visible` outline in `globals.css`)
  — this is a keyboard-driven app, focus state is not optional polish.
- Destructive actions (delete workspace, remove secret) use
  `variant="destructive"` styling and are never the default focused option
  in a dialog.
- Every theme is contrast-checked before it ships — see
  [Contrast](#contrast). A new theme is not done until body text, muted text,
  and the destructive/warning states have been measured against their real
  backgrounds in that theme.

### Known gaps

- Container and input borders sit at 1.3–1.5:1 against their surface in all
  three themes, below the 3:1 that WCAG 1.4.11 asks of controls. This is
  inherited from the original dark palette and kept consistent rather than
  fixed in two themes only; focus is what carries state, and `--ring` clears
  3:1 everywhere (Midnight 4.05, Daylight 6.69, Obsidian 5.92).
- The terminal's xterm palette (`src/features/terminal/registry.ts`) and the
  screenshot annotator's overlay colours
  (`src/features/issues/ShotAnnotator.tsx`) are hex literals outside the token
  system, so they do not follow the theme.
