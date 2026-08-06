# Design System

Visual language: Linear/Raycast-grade dark UI. Dark-first is not "dark mode
supported" — there is currently no light theme; `:root` carries the dark
palette directly (see `src/shared/styles/globals.css`).

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
`sidebar-*`, `chart-1..5`) are defined once in `globals.css` under
`@theme inline` and consumed as Tailwind utilities (`bg-background`,
`text-muted-foreground`, etc.) — never hex literals in components.

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
  tool-call activity rows.
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
