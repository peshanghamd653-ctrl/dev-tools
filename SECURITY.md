# Security Policy

DevOS holds API keys, gives an LLM gated access to your filesystem and shell,
and keeps an audit log of what it was allowed to do. Those are the parts worth
attacking, and the parts worth reporting on.

The full model — what is protected, how, and which guards carry which
guarantee — is [docs/security.md](docs/security.md). This file is the reporting
process and the boundary around it. Read the **Known and accepted** section
before filing: several things that look like holes are documented decisions,
and a report against one of them costs you time and tells us nothing new.

## Reporting a vulnerability

**Use GitHub's private vulnerability reporting.** On this repository:
**Security** → **Report a vulnerability**. That opens a private advisory
visible only to you and the maintainer. No email address is needed, and the
report is not public while it is being triaged.

Please do **not** open a public issue for a security problem first.

Useful in a report:

- What an attacker gains, and what they need in order to start.
- The version or commit you tested.
- Reproduction steps — ideally the smallest one that works. A failing test
  against the guard is the most useful form this can take (see
  [docs/testing.md](docs/testing.md); security behaviour here is expected to
  have a test that fails when the guard is removed).
- Whether you plan to disclose publicly, and when.

**What to expect.** This is a free project with one unpaid maintainer. Triage
is best effort and measured in days, not hours. There is no bug bounty and no
payment of any kind. Fixes land on `main` and go out in the next release; there
are no backports. Credit in the advisory and release notes if you want it.

## Supported versions

| Version | Supported |
|---|---|
| Latest release | ✅ |
| Anything older | ❌ |

Pre-1.0, single maintainer: only the newest release gets fixes. Upgrade rather
than expecting a patch on an old tag.

## Threat model

DevOS is a **single-user desktop application**. The model protects against
secrets leaking through the database file, backups, screenshots or logs; an AI
tool call escaping the granted project; an unintended destructive write through
the SQL editor; unattended outbound requests from a scheduled monitor; and
injection through webview content.

**Explicitly out of scope: an attacker who already controls the user's OS
account.** Windows Credential Manager hands the master key to any process
running as that user; the database sits under `%APPDATA%` with default ACLs;
any process with the user's token can read anything the user can read. DevOS
does not defend against that and does not claim to. A report whose first step
is "run code as the victim" is not a vulnerability in DevOS.

## In scope

Things that would be real findings:

- **Path containment bypass.** Any way for an AI tool, the code indexer, or the
  file explorer to read or write outside the granted project root — traversal,
  absolute paths, or escaping through a symlink or a Windows junction.
- **A gate that does not gate.** A mutating tool (`edit_file`, `write_file`,
  `run_command`, `save_memory`) executing without an approval; the
  write-tool grant surviving an app restart; an approval resolving for the
  wrong call.
- **An approval card that misrepresents what will happen.** This has been a
  real bug class here — a `write_file` card once read `New file: docs/notes.md`
  while the write followed a dangling junction out of the project. Consent to
  text that does not describe the action is not consent.
- **SQL write guards.** A write executing without the write toggle: statement
  smuggling past the one-statement-per-call check, a setter PRAGMA classified
  as a read, or any route around the read-only connection.
- **Secret exposure.** A stored value appearing in logs, emitted events, an IPC
  response, the audit log, a notification, an error message, or a URL or query
  string. Also: a credential sent to the wrong provider or host.
- **Audit log integrity.** Anything reachable from the webview that can write,
  edit, delete, or selectively prune `audit_log` — including a prompt injection
  writing its own alibi.
- **Webview and IPC boundary.** A CSP bypass, remote code execution in the
  webview, a Tauri capability granting more than the feature needs, or an IPC
  command reachable in a way its validation does not anticipate.
- **Indirect prompt injection with a real effect.** Content in a file the model
  merely *read* causing an unapproved side effect. The `save_memory` gap — model
  output writing durable, authoritative-looking text into every future
  conversation's system prompt — was exactly this shape and is now gated;
  anything of the same shape is in scope.
- **Unattended outbound requests.** A monitor or API request being creatable by
  anything other than a deliberate user action.
- **Release artifact integrity** — installer or update-manifest tampering, once
  signing and the updater are in place.

## Known and accepted

These are documented decisions, not oversights. Reporting them tells us what we
already wrote down. Each links to the reasoning.

- **The AI can edit files and run shell commands — that is the feature.** It
  requires two explicit grants and a per-call approval showing the full
  arguments, and it is off at every launch. "The AI ran a command I approved"
  is not a finding; "it ran one I did not" is.
  ([docs/security.md](docs/security.md#ai-tool-calling--implemented-m2))
- **`run_command` records its command line in the audit log**, truncated at 160
  characters. A credential embedded in an approved command (`curl -H
  "Authorization: Bearer …"`) therefore lands in the log in plaintext. This is
  the one deliberate exception to "record the action, not the payload" — the
  command *is* the action — and it is exactly the text the user was shown and
  approved. ([docs/security.md](docs/security.md#audit-log--implemented))
- **Screenshots hit the disk unredacted.** `issue_capture` writes the raw,
  full-resolution desktop to `<data-dir>/screenshots` as a plain PNG with
  default ACLs. Redaction is destructive in the *export*, but the source
  capture is what the annotator loads, so it exists on disk in the meantime.
  Retention is deliberately short, and the directory is documented rather than
  left to be discovered during a leak investigation.
- **Secret values cannot be read back from the UI, by construction.** No reveal
  button, no IPC path outward. That is the design, not a missing feature.
- **The SQL write toggle lets you drop your own tables.** Turning off a safety
  toggle and then using the capability it enables is the intended behaviour.
- **Monitors stop when the app closes**, deployments are read-only, and the
  screenshot flow does not attach the image. These are scope decisions
  ([ADR-0008](docs/adr/0008-in-process-watchers-notify-on-transitions.md),
  [ADR-0009](docs/adr/0009-deployments-read-only-no-write-actions.md)), not
  security failures.
- **The plugin runtime is not reachable.** `crates/devos-plugin` is a `wasmi`
  spike that is deliberately **not registered with the app**
  ([ADR-0010](docs/adr/0010-wasmi-interpreter-for-plugin-runtime.md)). Findings
  there are welcome and interesting, but nothing in it executes in a shipped
  build, so they are not vulnerabilities in DevOS today.
- **Installers are not code-signed yet**, so SmartScreen warns. Known; tracked
  in [docs/release-process.md](docs/release-process.md).
- **The Rust advisory gate is scoped to `x86_64-pc-windows-msvc`** (`deny.toml`
  `[graph] targets`). Advisories in crates that only ever compile for Linux or
  macOS — the GTK/X11 stack Tauri pulls into `Cargo.lock`, `xcb` behind `xcap`
  — are out of scope because they are not in the shipped binary. Unmaintained-
  crate advisories warn rather than fail, on purpose.
- **Third-party licence review is not done.** `cargo deny check licenses` is
  deliberately off. That is a distribution/compliance gap tracked in
  [docs/release-process.md](docs/release-process.md), not a security one.

## Out of scope

- Anything requiring an attacker who already executes code as the user.
- Missing hardening in a capability documented as deferred — see the "does not
  do" column in the [README](README.md) and the per-module deferrals in
  [docs/feature-roadmap.md](docs/feature-roadmap.md).
- Vulnerabilities in Anthropic, Google, Ollama, GitHub, Vercel or Docker
  themselves. Report those to them; if DevOS *uses* one unsafely, that part is
  in scope.
- Denial of service against the reporter's own machine.
- Automated scanner output with no demonstrated impact on this application.
- Social engineering, physical access, and anything requiring the user to
  deliberately approve the action being complained about.
