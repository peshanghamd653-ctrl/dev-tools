# UI Wireframes

Design language: Linear/Raycast-grade dark UI. Inter for text, JetBrains Mono
for paths/code. 8px spacing grid, `oklch` tokens in
`src/shared/styles/globals.css`.

## App shell (implemented, M0)

```
┌──────────┬───────────────────────────────────────────────────────────────┐
│ ▣ DevOS  │ Personal ▾                        [ ⌘ Search or run…  Ctrl K ]│
│          ├───────────────────────────────────────────────────────────────┤
│ ▸ Dash   │                                                               │
│ ▸ Projects                     ROUTED CONTENT                            │
│ ▸ Settings                                                               │
│ ──────── │                                                               │
│ COMING   │                                                               │
│ ░ Terminal M1                                                            │
│ ░ Git      M1                                                            │
│ ░ AI       M1                                                            │
│ ░ Docker   M3                                                            │
│          │                                                               │
│ ⟨ collapse│                                                              │
└──────────┴───────────────────────────────────────────────────────────────┘
```

- Sidebar collapses to icon rail (Ctrl+B); planned modules render disabled
  with milestone badges — visible roadmap, no fake screens.
- Workspace switcher lives in the topbar; palette trigger shows its shortcut.

## Command palette (implemented, M0)

```
┌───────────────────────────────────────────┐
│ 🔍 Search pages and commands…             │
├───────────────────────────────────────────┤
│ NAVIGATION                                │
│  ▸ Dashboard                       Ctrl+1 │
│  ▸ Projects                        Ctrl+2 │
│ ACTIONS                                   │
│  + Create workspace                       │
│  + Add project                            │
│ MODULES  (from kernel CommandRegistry)    │
│  ⚙ Create Workspace              core     │
└───────────────────────────────────────────┘
```

## Dashboard (implemented, M0)

```
┌ Workspace name ────────────────────────────────────────────┐
│ [Projects n] [Workspaces n] [Startup ms] [Version]         │
│ ┌ Recent projects ───────────┐ ┌ Quick actions ──────────┐ │
│ │ name        path           │ │ + Add a project         │ │
│ │ …                          │ │ + Create a workspace    │ │
│ │ (empty → CTA)              │ │ ▸ Browse projects       │ │
│ └────────────────────────────┘ └─────────────────────────┘ │
└────────────────────────────────────────────────────────────┘
```

Dashboard grows per milestone: M1 adds git status + running terminals; M3
containers; M4 deployments/uptime. Cards appear only when their module is
active — never placeholder data.

## Planned layouts (M1)

Terminal — tab strip per session, split panes, AI suggestion row above input:

```
┌ terminals ─ [zsh ×] [build ×] [+] ────────── [split ▯▯] ┐
│ $ pnpm build                                            │
│ …stream…                                                │
├─────────────────────────────────────────────────────────┤
│ ⌁ suggested: pnpm build --filter app   (Tab to accept)  │
└─────────────────────────────────────────────────────────┘
```

Git — three-pane: changes list · diff viewer · commit box with AI message
button. AI chat — right dock or full page, streaming markdown, model picker
(Claude / Ollama), context chips showing which files are attached.
