# 0009 — Deployment integration ships read-only, with no write actions

Status: accepted
Date: 2026-08-06

## Context

M4's deployment item was scoped as "Vercel first." The obvious shape of a
deployment module is a list of deployments with buttons beside them:
redeploy, promote to production, roll back, delete. Those buttons are also
the entire reason this integration is more dangerous than anything DevOS
has shipped so far.

Every risky surface up to now has its blast radius on the user's own
machine. A bad statement in the SQL editor damages a file the user picked;
a `run_command` tool call runs in a project root the user granted; a
monitor sends a request to a URL the user typed. A deployment write action
does not stop there — it changes what the public sees, from a list where
two projects sit one row apart and a stale deployment looks exactly like a
current one. And there is no undo inside DevOS: recovering from a wrong
promote or a wrong rollback means opening the Vercel dashboard, which is
where the action should probably have been performed in the first place.

## Decision

Ship the module **read-only**. `devos-deploy` lists Vercel projects and
their recent deployments — state, target, URL, commit message, timestamp —
and offers nothing that changes anything on Vercel. No triggering, no
promoting, no rolling back, no deleting. Those calls are not implemented at
all; they are not present behind a toggle or a confirmation.

Three commands, all reads: `deploy_configured`, `deploy_projects`,
`deploy_list` (see [ipc-contracts.md](../ipc-contracts.md)). The crate
itself never touches the secret store — its functions take
`(token, base_url, …)` and the command layer resolves `vercel_token` from
app state, which also makes `base_url` injectable, so every test runs
against a local one-shot server and no test reaches the real API.

This is deliberately the same move [ADR-0005](0005-read-only-tools-first-with-explicit-grant.md)
made for AI tool calling: ship the read half, get the surface right, and
let writes arrive later behind a gate designed for them rather than
inherited by accident. That is precedent rather than analogy — the writes
did land there, and they landed with per-call approval.

## Alternatives considered

- **Ship the write actions behind a confirmation dialog** — what everyone
  expects a deployment tool to do, and the gate is the weakest one
  available on the surface that deserves the strongest. "Are you sure?" is
  answered yes reflexively, and it is only meaningful if the dialog names
  the exact project and environment correctly — which is a display problem
  that has to be solved before the button can exist, not alongside it.
- **Ship redeploy only, since it is "additive"** — a redeploy destroys
  nothing on the API: it builds again from the same commit. But against a
  production target it replaces what is live, spends build minutes, and can
  turn a working deployment into a failing one when something upstream
  moved (a yanked dependency, a rotated env var). Additive for the API is
  not additive for the user's site.
- **Ship rollback, but restrict it to preview deployments** — a rule the UI
  would have to enforce from `target`, which is nullable and frequently
  absent for CLI deploys. A safety rule that depends on a field the API
  does not reliably populate is not a safety rule.
- **No deployment module until writes can be done properly** — rejected
  from the other direction. Read-only visibility is useful on its own (did
  the push build, which commit is live, what failed), it costs nothing
  worse than an outbound GET, and it is how the module earns the right to
  be trusted with more.

## Consequences

- **DevOS does not replace the Vercel dashboard — you still go there to
  ship.** Anything that changes a deployment happens in the browser. This
  module answers "what is the state of my deploys" and nothing else, and
  the sidebar entry should not be read as promising more.
- The restraint lives in DevOS's code, not in the credential. DevOS asks for
  an ordinary Vercel API token, so the credential it holds can do whatever
  that token can do — including every action this ADR declines to
  implement. What bounds the blast radius is that no code path exists to
  make those calls, the same property ADR-0005 leaned on before write tools
  existed.
- **There is no read-only Vercel token to lean on instead.** Vercel scopes
  tokens by *reach* — account, team, or a single project — not by
  *permission*; granular permissions were still in private beta as of
  August 2026. So a project-scoped token is the narrowest credential a user
  can hand DevOS: it bounds *which project* a leaked or misused token can
  affect, but not *what* it can do to that project. Worth recommending in
  the secrets UI, and worth revisiting if permission scoping ships.
- **Nothing is persisted.** Deployment data is read live per request, like
  the Docker module, so the planned `deployments` table was never created.
  That keeps the module stateless and honest (the UI shows what Vercel says
  right now), and it forecloses the obvious next feature: notifying on a
  failed deploy needs a previous state to compare against, which is exactly
  the shape [ADR-0008](0008-in-process-watchers-notify-on-transitions.md)
  gave the monitor. A deploy watcher is a storage decision, not a polling
  loop.
- Failure modes stay legible: not-configured (no token stored),
  auth-rejected (401/403 — a token is present but bad or expired), and
  generic API/transport failure are distinct errors, so the UI can say
  which one actually happened instead of showing an empty list three ways.
- **Before any write action lands**, two things have to exist. First, the
  per-call approval gate the AI write tools already use — the pending
  action shown with its full arguments, an explicit approve, deny on
  timeout ([security.md](../security.md)) — not a confirm dialog. Second, an
  unambiguous display of exactly which project and which environment the
  action targets, resolved from data the API actually returns rather than
  from a nullable `target`. Neither is speculative work; both are
  prerequisites, and this ADR should be superseded rather than quietly
  amended when they are built.
