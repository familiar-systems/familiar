# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

Covers `apps/platform/` only: the `familiar-systems-platform` Axum binary. Overrides the repo-root CLAUDE.md for anything under this directory.

## Today vs. the PRD

The [App Server PRD](../../docs/plans/2026-04-11-app-server-prd.md) specifies auth, campaign CRUD, membership, a routing table, shard heartbeats and leases, checkout orchestration, billing, and a public showcase. **The shipped surface is `GET /health` and `GET /me`.** Everything else is greenfield: treat the PRD as the spec, not as a description of code to extend.

Related context for architectural decisions: [Deployment Architecture](../../docs/plans/2026-03-30-deployment-architecture.md).

## Commands

Prefer the workspace-wide `mise` tasks. They are fast, and they cover the shared crates this binary depends on (`familiar-systems-app-shared`) plus the other consumer of those crates (`apps/campaign`), so a change to an auth type is validated against every site that uses it, not just this one.

```bash
mise run test                                                                        # whole workspace (Rust + TS + Python)
mise run lint
mise run typecheck
mise run format
mise run dev:platform                                                                # run the server locally
```

Drop to crate-scoped `cargo` only when you want to target a single test case during iteration:

```bash
cargo test -p familiar-systems-platform --test auth_test                             # one integration binary
cargo test -p familiar-systems-platform --test auth_test email_collision_returns_409 # one case
```

Re-run `mise run test` before you consider the work done.

`cargo run -p familiar-systems-platform` alone will panic: `Config::from_env` requires `HANKO_API_URL` and `CORS_ORIGINS`. `mise run dev:platform` injects both (see `mise.toml`).

## Where to read before editing

Rich rustdoc already lives at each site. Read the doc block first; do not duplicate it here or elsewhere.

| If you are touching... | Read |
| --- | --- |
| auth types, validator, or the wire/domain/api split | `crates/app-shared/src/auth/mod.rs` (module doc), `auth/domain.rs::HankoClaims` |
| the auth extractor, upsert, `EmailConflict` path | `src/middleware/auth.rs` |
| `AppError` variants, HTTP-body policy, log `kind` labels | `src/error.rs` (comment above `IntoResponse`) |
| routes, CORS, Tower layer ordering, request-id / tracing span | `src/routes/mod.rs` |
| `users` table, `id = Hanko subject`, email invariants | `src/entities/users.rs`, `src/migrations/m20260417_000001_create_users.rs` |
| required env vars and why `CORS_ORIGINS` is mandatory even same-origin | `src/config.rs` |

Two facts worth surfacing because they span files:

- **Routes are post-strip.** The reverse proxy removes `/api` before requests arrive; declare `/me`, never `/api/me`.
- **`users.id` is the Hanko subject (UUID).** Parsed at the auth boundary in `HankoClaims::try_from`; foreign keys should reference this column directly.

## Testing

`tests/common::spawn_app()` gives every test a fresh `sqlite::memory:` DB with migrations applied, a `wiremock` Hanko, and the real router on an ephemeral port. Each file under `tests/` compiles as its own binary. `config.rs` env-var tests use `#[serial]` from `serial_test`.

Convention across auth-failure tests (not obvious from any one file): **assert both the generic response body and that zero rows were written.** The hardening guarantee is that rejected requests never persist state and never leak internal detail; a status-only assertion will not catch regressions on either.

## Adding code

- **New route**: post-strip path, new module under `src/routes/`, register in `routes/mod.rs::router`. Take `AuthenticatedUser` as a parameter to require auth.
- **New config value**: choose panic-on-missing or defaulted; add a `#[serial]` test.
- **New `AppError` variant**: follow the policy comment in `error.rs` (generic public message, distinct `kind` for logs).
- **New API response type**: live in `crates/app-shared/src/auth/api.rs` (or sibling) with `#[derive(Serialize, TS, ToSchema)]` and `#[ts(export_to = "types-app/src/generated/...")]`. Run `mise run generate-types`.
- **New migration**: new file under `src/migrations/`, register in `migrations/mod.rs`. Every test migrates from empty, so an "only works on populated DB" migration is a real bug.
