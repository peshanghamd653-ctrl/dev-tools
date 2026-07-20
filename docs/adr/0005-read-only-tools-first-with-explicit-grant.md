# 0005 — AI tool calling ships read-only-first, gated by explicit grant

Status: accepted
Date: 2026-07-20

## Context

M2 needed the AI assistant to do more than answer from context the user
pasted in — it needed to read the actual project. The natural next step
after "read files" is "edit files" and "run commands," which is
categorically more dangerous (data loss, arbitrary code execution).

## Decision

Ship only three **read-only** tools (`read_file`, `list_dir`, `find_files`),
scoped to one canonicalized project root with traversal rejected, and make
them **inert by default** — the tools list sent to the model is empty
unless the user has explicitly toggled a per-conversation "Tools" chip on.
`edit_file` and `run_command` are deliberately deferred, and when they
land, planned to require **per-call approval** (a dialog per action), not
just a standing conversation-level grant.

## Alternatives considered

- **Ship read+write+execute together** — more capable in one pass, but
  conflates two very different risk levels (reading a file vs. deleting or
  overwriting one, vs. running an arbitrary shell command) under one
  approval gesture. A user granting "let it read my code" should not
  accidentally also be granting "let it run commands."
- **No standing grant — approve every single tool call, even reads** — safer
  still, but for read-only, side-effect-free operations this is friction
  without a corresponding safety benefit; it would make the feature
  annoying enough to not get used, which has its own cost (the user falls
  back to copy-pasting file contents into chat manually, which is worse for
  privacy, not better).

## Consequences

- The riskiest capabilities (write, execute) simply don't exist yet — there
  is no code path to audit for "what if the model decides to delete
  something," because deletion isn't possible through this surface at all.
- The approval UX precedent (standing grant for read-only, per-call
  approval once write/execute exist) is now the model M3+ tool work is
  expected to extend from — see [security.md](../security.md) and
  [agents.md](../agents.md).
- Every tool call surfaces in the chat UI live (name, arguments,
  success/failure) — transparency substitutes for per-call friction on
  the read-only path specifically because the action is safe to have
  already happened by the time the user sees it.
