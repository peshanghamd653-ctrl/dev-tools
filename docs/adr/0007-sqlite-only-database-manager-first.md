# 0007 — Database manager ships SQLite-only behind a driver-shaped abstraction

Status: accepted
Date: 2026-08-06

## Context

M3's database manager was scoped as "SQLite/Postgres/MySQL; schema
explorer, SQL editor." Those three are one feature only on the roadmap
line. In the crate they are three `sqlx` driver features, three connection
and credential flows, three schema-introspection dialects (`sqlite_master`
vs `information_schema` vs `SHOW`), and three sets of type mapping into a
string grid. Two of the three also need a running server to test against
— which means the parts that are hardest to get right (credential
storage, TLS, connection lifetime) are exactly the parts that would ship
least exercised.

## Decision

Ship **SQLite only**, and shape the persistence and DTOs so the other
drivers slot in later without a rewrite:

- `db_connections` carries a `driver` column from day one, holding
  `sqlite` for every row that exists today.
- `DbConnection` / `DbSchema` / `QueryResult` are dialect-neutral —
  `QueryResult` in particular returns every cell as `string | null`, so no
  driver's native type set leaks into the contract.
- `sqlx`'s `postgres` and `mysql` features are **not** enabled, and no
  server code path exists. The abstraction is in the data shapes, not in a
  trait with one implementation.

## Alternatives considered

- **Ship all three drivers at once** — the advertised scope, and the only
  option that makes the module competitive with a real DB GUI. Rejected
  because a half-tested Postgres path is worse than an absent one: absent
  is honest and users keep their existing tool, whereas half-tested looks
  supported, gets pointed at a production database, and fails as a wrong
  result or a mishandled password rather than a clear "not supported yet."
  It also drags in the whole credential surface (host/port/user/password/
  TLS) that must round-trip through `secrets` and stay redacted at the IPC
  boundary — real work, unjustifiable while it can't be exercised.
- **Define the `DbDriver` trait now, with SQLite as its only
  implementation** — looks like preparation, is actually guessing. The
  trait would be designed against one dialect and imagined for two others
  whose introspection genuinely differs; the odds of the guess surviving
  contact with a real Postgres implementation are poor. Shaping the DTOs
  (cheap, and verifiable today) and deferring the trait until there is a
  second implementation to generalize *from* costs less than a wrong trait
  that has to be unwound.
- **Shell out to `psql`/`mysql`, the way [ADR-0001](0001-shell-out-to-git-cli.md)
  does for git** — tempting for the same reasons, and still open. But
  unlike `git`, these clients are not reliably installed on a developer's
  machine, and their output is formatted for humans rather than for
  parsers. The argument that carried ADR-0001 (behave exactly like the
  user's own tool) doesn't transfer.

## Consequences

- **DevOS does not replace DBeaver, TablePlus, or pgAdmin.** For anyone
  whose daily work is a Postgres or MySQL instance, this module is not yet
  a reason to use DevOS — it is a SQLite browser. That is the cost, stated
  plainly rather than hedged on the roadmap.
- Compile time and binary size stay where they are. The `postgres` and
  `mysql` driver features each pull in their own wire protocol and TLS
  stack; not enabling them is the single largest build-cost decision in
  this module.
- No credential handling exists in `devos-db` at all — nothing to leak,
  nothing to redact, nothing to audit. `db_connections` stores a
  canonicalized file path because that is genuinely all SQLite needs.
- The `driver` column holds one distinct value until a second driver
  lands. That is deliberate dead weight, and cheaper than an ALTER later.
- **Migration path** when Postgres arrives: enable the `sqlx` feature, add
  a `DbDriver` trait with per-dialect introspection, add a `secret_id`
  column to `db_connections` pointing into `secrets`, and extend the
  connect arguments with host/port/user/database. The IPC command names,
  the `QueryResult` shape, and the read/write gate all survive unchanged —
  with one known exception: the read-only connection, the load-bearing line of
  defence on the read path (see [security.md](../security.md)), has no
  Postgres equivalent. A Postgres read path would need a read-only
  transaction instead, so that defence has to be re-established per
  driver rather than inherited.
