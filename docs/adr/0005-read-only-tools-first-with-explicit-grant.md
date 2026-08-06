# 0005 — AI tool calling ships read-only-first, gated by explicit grant

Status: accepted — amended 2026-08-06, see [Update](#update-2026-08-06)
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

- At the time of this decision the riskiest capabilities (write, execute)
  did not exist — there was no code path to audit for "what if the model
  decides to delete something," because deletion was not possible through
  this surface at all. **This is no longer true; see the Update below.**
- The approval UX precedent (standing grant for read-only, per-call
  approval once write/execute exist) is now the model M3+ tool work is
  expected to extend from — see [security.md](../security.md) and
  [agents.md](../agents.md).
- Every tool call surfaces in the chat UI live (name, arguments,
  success/failure) — transparency substitutes for per-call friction on
  the read-only path specifically because the action is safe to have
  already happened by the time the user sees it.

## Update (2026-08-06)

The deferred capabilities shipped later in M2, along the path this ADR
planned. The decision was not reversed — it was completed — but the
consequence above described a state of the world that no longer holds, and
an ADR that understates the current attack surface is worse than no ADR.
What exists today:

- `edit_file`, `write_file` and `run_command` are implemented in
  `src-tauri/src/tools.rs`, alongside the original read tools (which also
  gained `search_code` and `save_memory`).
- They sit behind a **second** standing grant, separate from the read
  chip — the user turns on "Edits & commands" deliberately, so granting
  reads still never implies granting writes.
- **Every individual call is additionally gated by per-call approval**
  (`src-tauri/src/approvals.rs`): the model's request is surfaced with its
  arguments, and the call blocks on the user's answer, with a timeout that
  denies rather than allows. That is the per-call dialog this ADR named as
  a precondition, and it was not skipped.
- The path-containment guard is shared with the file explorer
  (`src-tauri/src/pathsafe.rs`), so the traversal property cannot drift
  apart between the two surfaces.

There *is* now a code path to audit for destructive action. See
[security.md](../security.md) and [agents.md](../agents.md), which have
described the write tier correctly throughout.

Two corrections from a later security review (2026-08-06/07), both of which
this ADR's framing contributed to:

- **"Read-only tier" stopped being literally true and nobody noticed.**
  `save_memory` was added to the read tier, where the gate did not apply —
  it was checked inside the `edit_file | write_file | run_command` match
  arm, so a fifth mutating tool added elsewhere inherited nothing. What it
  writes is injected into the system prompt of every later conversation,
  which makes it a more durable capability than a single file edit. The
  gate is now keyed off an explicit `MUTATING_TOOLS` list checked before
  dispatch, so the property is "mutating implies approval" rather than
  "these three names imply approval". The tier split stands — the second
  grant means filesystem and shell, which remembering a fact is not.
- **The write grant is no longer persisted.** It had been restored from
  localStorage, so "off by default" held only on first run. It is now
  session-scoped, and a grant left by an older build is forced off on
  rehydration. Per-call approval still guards each action, but the grant
  decides whether those tools are offered to the model at all, and that is
  the cheaper thing to keep narrow.
