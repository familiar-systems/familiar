# App Server Walking Skeleton: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the minimum end-to-end slice proving browser → Hanko Cloud → Axum → SeaORM → SQLite → browser, deployed on localhost and per-PR preview environments.

**Architecture:** SPA (`apps/web`) authenticates against Hanko Cloud directly (no proxy). The browser attaches the Hanko session JWT as a Bearer header to `/api/me` on the platform server (`apps/platform`, Axum + SeaORM + SQLite). The platform validates the JWT by POSTing it to Hanko's `/sessions/validate`, upserts a `users` row keyed by `claims.subject`, and returns it. Hanko tenant URL is config (not a secret).

**Tech Stack:** Rust stable + edition 2024; Axum 0.8; SeaORM 1.1; SQLite via `sqlx-sqlite`; `reqwest` with `rustls`; `tower-http` CORS; React 19 + Vite 7; `@teamhanko/hanko-elements` 1.x; k3s + Traefik; Scaleway Container Registry; Pulumi (Python).

---

## Context

Why this change: `CLAUDE.md` documents the app as pre-implementation; `apps/platform` and `apps/campaign` currently contain only `fn main() {}`. We need the auth + DB walking skeleton in place before any real feature (campaign CRUD, routing table, shard coordination, billing) can land. All subsequent plans (`2026-04-11-app-server-prd.md`) assume this skeleton exists.

Design source: `INITIAL_APP.md` (committed as an untracked file at repo root). This plan decomposes its §12 commit sequence into bite-sized TDD tasks and resolves its two flagged open questions.

Resolved open questions (from context7):
- **Hanko JS SDK session-token accessor:** `hanko.getSessionToken()` returns the JWT or `null`. Source: `docs.hanko.io/resources/frontend-sdk`, `docs.hanko.io/guides/hanko-elements/using-frontend-sdk`.
- **SeaORM feature flags:** For our stack (sea-orm 1.1.x stable, which cargo prefers over the 2.0 RC channel): `sqlx-sqlite`, `runtime-tokio-rustls`, `macros`, `with-uuid`, `with-chrono`. These names are identical in both 1.1 and 2.0 RC lines. Source: `docs.rs/sea-orm/1.1`.

Verified current-state facts from exploration (not re-verified in tasks):
- `apps/platform/src/main.rs` is literally `fn main() {}`.
- `[workspace.dependencies]` already declares `serde`, `serde_json`, `tokio`, `ts-rs`, `utoipa`, `uuid`, `nanoid`, `loro`.
- `mise.toml` has a `dev:platform` task with no `env` block.
- `packages/types-app` and `packages/types-campaign` are wired and committed. Adding `#[ts(export, export_to = "types-app/src/generated/…")]` to new Rust types automatically emits TypeScript via `mise run generate-types`.
- Current Vite proxy maps `/api` → `localhost:3001` (campaign). Needs changing to `:3000` (platform) for this slice.
- Existing preview manifests are `namespace.yaml`, `registry-pull-secret.yaml`, `deployment.yaml`, `service.yaml`, `ingress.yaml`. The latter three are site-only and will be renamed with `site-` prefix.
- `config.py` has `PRODUCTION_DOMAINS = ["loreweaver.no", "familiar.systems"]`. Both `api.familiar.systems` AND `app.familiar.systems` need adding (confirmed with user).
- `config.py` constants: `WILDCARD_CERT_SECRET = "preview-wildcard-tls"`, `REGISTRY_PULL_SECRET = "scaleway-registry"`.

---

## Global Conventions (Non-Negotiable)

These come from user preferences stored across sessions. Every task inherits them.

1. **No `Co-Authored-By` trailers** in any git commit.
2. **No em-dashes** (—) or double-dashes (--) in prose or commit messages. Rephrase instead.
3. **Use `git mv` for tracked-file renames.** Never plain `mv`.
4. **Version pins in this plan are placeholders.** Run `cargo add` / `pnpm add` to pick the current upstream version. Never copy a version string from this document.
5. **Fix every warning on sight**, including pre-existing ones in files you touch.
6. **User runs Pulumi commands themselves.** Tasks editing Pulumi code do NOT run `pulumi up/preview/destroy`. Prompt the user when a Pulumi diff is ready for review.
7. **Python infra lint/format in one pass:** `cd infra/pulumi-cloud && uv run ruff check --fix . && uv run ruff format . && uv run basedpyright`. Never run `--check` separately first.
8. **Automate verification.** Every task's verification step is a command the agent runs, not an instruction for the user.
9. **Prefer sum types (enums) and state machines** over mutable fields where feasible.
10. **Non-secret config goes in Pulumi constants / mise.toml**, never Scaleway Secrets Manager. (`HANKO_API_URL`, `CORS_ORIGINS`, etc. are public.)
11. **Commit message style:** conventional-commit prefix (`feat:`, `fix:`, `test:`, `chore:`, `deps:`, `infra:`, `ci:`, `build:`), scope in parens (`feat(platform): …`), no em-dashes, no Co-Authored-By.

---

## File Structure Overview

```
Cargo.toml                                # + workspace deps
crates/app-shared/
  Cargo.toml                              # + reqwest, thiserror, chrono; dev + wiremock
  src/
    lib.rs                                # + pub mod auth;
    auth.rs                               # NEW: HankoSessionValidator, HankoClaims, HankoEmail, AuthError

apps/platform/
  Cargo.toml                              # + axum, sea-orm, sea-orm-migration, tower-http, tracing, garde
  Dockerfile                              # NEW: multi-stage Rust build
  .dockerignore                           # NEW
  src/
    main.rs                               # boots tracing, Config, DB, migrations, router
    lib.rs                                # NEW: re-exports router + types for integration tests
    config.rs                             # NEW: Config::from_env
    state.rs                              # NEW: AppState { db, validator, config }
    error.rs                              # NEW: AppError + IntoResponse
    middleware/
      mod.rs                              # NEW
      auth.rs                             # NEW: AuthenticatedUser extractor
    routes/
      mod.rs                              # NEW: router()
      health.rs                           # NEW: GET /health
      me.rs                               # NEW: GET /me (authenticated)
    entities/
      mod.rs                              # NEW
      users.rs                            # NEW: SeaORM entity
    migrations/
      mod.rs                              # NEW: Migrator
      m20260417_000001_create_users.rs    # NEW: single migration
  tests/
    common/mod.rs                         # NEW: spawn_app + MockHankoServer helpers
    health_test.rs                        # NEW
    migration_test.rs                     # NEW
    auth_test.rs                          # NEW: 5 integration tests

apps/web/
  package.json                            # + @teamhanko/hanko-elements
  vite.config.ts                          # /api proxy target :3001 -> :3000
  nginx.conf                              # NEW: SPA fallback
  Dockerfile                              # NEW: node build + nginx serve
  .dockerignore                           # NEW
  .env.local.example                      # NEW: VITE_HANKO_API_URL + VITE_API_BASE_URL
  src/
    custom.d.ts                           # NEW: JSX decl for <hanko-auth>
    vite-env.d.ts                         # NEW: typed import.meta.env
    App.tsx                               # branch on pathname: /login vs /
    login.tsx                             # NEW: mounts <hanko-auth>
    home.tsx                              # NEW: fetch /api/me
    lib/hanko.ts                          # NEW: Hanko client + getSessionToken

infra/pulumi-cloud/
  config.py                               # + api.familiar.systems, app.familiar.systems, HANKO_API_URL_DEV/PROD
  k8s.py                                  # + platform PV/PVC/Deployment/Service/Ingress

infra/k8s/preview/
  deployment.yaml -> site-deployment.yaml # git mv
  service.yaml    -> site-service.yaml    # git mv
  ingress.yaml    -> site-ingress.yaml    # git mv
  platform-pv.yaml                        # NEW
  platform-pvc.yaml                       # NEW
  platform-deployment.yaml                # NEW
  platform-service.yaml                   # NEW
  platform-ingress.yaml                   # NEW
  web-deployment.yaml                     # NEW
  web-service.yaml                        # NEW
  web-ingress.yaml                        # NEW

.github/workflows/
  deploy-preview.yml                      # paths filter + matrix build + ordered apply + 3-URL comment
  cleanup-preview.yml                     # 3-image tag cleanup + PV cleanup

mise.toml                                 # dev:platform gets env block
```

---

## Task List

Tasks are ordered by dependency. Tasks with disjoint scope (e.g., frontend vs infra) may be executed in parallel worktrees if using subagent-driven mode, but sequential execution is safe.

### Task 1: Add workspace deps for `app-shared`

**Goal:** Add `reqwest`, `thiserror`, `chrono` (with serde) to `[workspace.dependencies]` and wire them into `crates/app-shared/Cargo.toml`.

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/app-shared/Cargo.toml`

**Depends on:** none

**Steps:**
- [ ] 1. Run `cargo add --package familiar-systems-app-shared reqwest --no-default-features --features json,rustls thiserror` from repo root. Cargo adds these to both `[workspace.dependencies]` (root) and `app-shared` (`workspace = true`), following the convention already used for `serde`/`tokio`. (In reqwest 0.13.x the TLS-via-rustls feature is named `rustls`; older versions used `rustls-tls`.)
- [ ] 2. Run `cargo add --package familiar-systems-app-shared chrono --features serde`.
- [ ] 3. Open root `Cargo.toml` and verify `reqwest`, `thiserror`, `chrono` entries exist under `[workspace.dependencies]`. Open `crates/app-shared/Cargo.toml` and verify they reference `workspace = true`.
- [ ] 4. Run `mise run typecheck:rust`. Expect PASS (empty code still compiles).
- [ ] 5. Commit:
```
deps(app-shared): add reqwest, thiserror, chrono for Hanko validator
```

---

### Task 2: Add workspace deps for `platform`

**Goal:** Add axum, sea-orm, sea-orm-migration, tower-http, tracing, tracing-subscriber, garde.

**Files:**
- Modify: `Cargo.toml`
- Modify: `apps/platform/Cargo.toml`

**Depends on:** Task 1

**Steps:**
- [ ] 1. Run `cargo add --package familiar-systems-platform axum`.
- [ ] 2. Run `cargo add --package familiar-systems-platform sea-orm --features sqlx-sqlite,runtime-tokio-rustls,macros,with-uuid,with-chrono` (these flag names match sea-orm 1.1.x stable, which cargo prefers over 2.0 RC).
- [ ] 3. Run `cargo add --package familiar-systems-platform sea-orm-migration --features sqlx-sqlite,runtime-tokio-rustls`.
- [ ] 4. Run `cargo add --package familiar-systems-platform tower-http --features cors,trace`.
- [ ] 5. Run `cargo add --package familiar-systems-platform tracing tracing-subscriber --features env-filter`. (Second invocation because `--features` is per-crate in `cargo add`.)
- [ ] 6. Run `cargo add --package familiar-systems-platform garde --features derive,email`.
- [ ] 7. Run `cargo add --package familiar-systems-platform tokio --features macros,rt-multi-thread serde serde_json thiserror chrono uuid`.
- [ ] 8. Run `mise run typecheck:rust`. Expect PASS.
- [ ] 9. Commit:
```
deps(platform): add axum, sea-orm, tower-http, tracing, garde
```

---

### Task 3: `auth.rs` claim + error types

**Goal:** Establish `HankoClaims`, `HankoEmail`, `AuthError` with deserialization and Display tests.

**Files:**
- Create: `crates/app-shared/src/auth.rs`
- Modify: `crates/app-shared/src/lib.rs`

**Depends on:** Task 1

**Steps:**
- [ ] 1. Add `pub mod auth;` to `crates/app-shared/src/lib.rs`.
- [ ] 2. Create `crates/app-shared/src/auth.rs` with failing tests:
```rust
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct HankoClaims {
    pub subject: String,
    pub email: Option<HankoEmail>,
    pub expiration: DateTime<Utc>,
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HankoEmail {
    pub address: String,
    pub is_primary: bool,
    pub is_verified: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("missing authorization header")]
    MissingHeader,
    #[error("hanko rejected session: {0}")]
    SessionRejected(String),
    #[error("hanko request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_deserialize_from_hanko_response_shape() {
        let raw = r#"{"subject":"sub-1","email":{"address":"a@b.com","is_primary":true,"is_verified":true},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert_eq!(c.subject, "sub-1");
        let email = c.email.unwrap();
        assert_eq!(email.address, "a@b.com");
        assert!(email.is_primary);
        assert!(email.is_verified);
    }

    #[test]
    fn claims_deserialize_with_null_email() {
        let raw = r#"{"subject":"sub-1","email":null,"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert!(c.email.is_none());
    }

    #[test]
    fn claims_deserialize_with_absent_email() {
        let raw = r#"{"subject":"sub-1","expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert!(c.email.is_none());
    }

    #[test]
    fn auth_error_display_is_stable() {
        assert_eq!(AuthError::MissingHeader.to_string(), "missing authorization header");
    }
}
```
- [ ] 3. Run `cargo test -p familiar-systems-app-shared auth`. Expect PASS.
- [ ] 4. Commit:
```
feat(app-shared): add Hanko claim and auth error types
```

**Notes:** Types are intentionally plain structs with `pub` fields. No `Default`, no builder. Docs source: `docs.hanko.io/api-reference/public/session-management/validate-a-session`.

---

### Task 4: `HankoSessionValidator` happy path

**Goal:** Implement `HankoSessionValidator::validate` against a mock HTTP server, covering the success path.

**Files:**
- Modify: `crates/app-shared/src/auth.rs`
- Modify: `crates/app-shared/Cargo.toml` (dev-deps)

**Depends on:** Task 3

**Steps:**
- [ ] 1. Run `cargo add --package familiar-systems-app-shared --dev wiremock tokio --features macros,rt`.
- [ ] 2. Append to `auth.rs`:
```rust
pub struct HankoSessionValidator {
    client: reqwest::Client,
    api_url: String,
}

#[derive(serde::Deserialize)]
struct ValidateResponse {
    is_valid: bool,
    claims: Option<HankoClaims>,
}

#[derive(serde::Serialize)]
struct ValidatePayload<'a> {
    session_token: &'a str,
}

impl HankoSessionValidator {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self { client: reqwest::Client::new(), api_url: api_url.into() }
    }

    pub async fn validate(&self, token: &str) -> Result<HankoClaims, AuthError> {
        let url = format!("{}/sessions/validate", self.api_url.trim_end_matches('/'));
        let resp = self.client
            .post(&url)
            .json(&ValidatePayload { session_token: token })
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AuthError::SessionRejected(format!("HTTP {}", resp.status())));
        }
        let body: ValidateResponse = resp.json().await?;
        if !body.is_valid {
            return Err(AuthError::SessionRejected("is_valid=false".into()));
        }
        body.claims.ok_or_else(|| AuthError::SessionRejected("no claims".into()))
    }
}
```
- [ ] 3. Append a happy-path test inside `mod tests`:
```rust
#[tokio::test]
async fn validate_returns_claims_on_is_valid_true() {
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};
    let srv = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": "u-1",
                "email": {"address": "x@y.com", "is_primary": true, "is_verified": true},
                "expiration": "2099-01-01T00:00:00Z",
                "session_id": "sess-1"
            }
        })))
        .mount(&srv).await;
    let v = HankoSessionValidator::new(srv.uri());
    let c = v.validate("tok").await.unwrap();
    assert_eq!(c.subject, "u-1");
    assert_eq!(c.session_id, "sess-1");
}
```
- [ ] 4. Run `cargo test -p familiar-systems-app-shared`. Expect PASS.
- [ ] 5. Commit:
```
feat(app-shared): implement HankoSessionValidator::validate
```

**Notes:** The wire shape (POST + JSON body with `session_token`) matches Hanko's official Rust quickstart at `docs.hanko.io/quickstarts/backend/rust`. OpenAPI also documents GET + Bearer; the quickstart is authoritative for maintenance.

---

### Task 5: Validator error-path tests

**Goal:** Cover HTTP 4xx, `is_valid=false`, and missing-claims branches.

**Files:**
- Modify: `crates/app-shared/src/auth.rs`

**Depends on:** Task 4

**Steps:**
- [ ] 1. Append three tests to `mod tests`:
```rust
#[tokio::test]
async fn validate_rejects_on_http_401() {
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};
    let srv = MockServer::start().await;
    Mock::given(method("POST")).and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&srv).await;
    let v = HankoSessionValidator::new(srv.uri());
    assert!(matches!(v.validate("t").await, Err(AuthError::SessionRejected(_))));
}

#[tokio::test]
async fn validate_rejects_on_is_valid_false() {
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};
    let srv = MockServer::start().await;
    Mock::given(method("POST")).and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"is_valid": false})))
        .mount(&srv).await;
    let v = HankoSessionValidator::new(srv.uri());
    assert!(matches!(v.validate("t").await, Err(AuthError::SessionRejected(_))));
}

#[tokio::test]
async fn validate_rejects_when_claims_missing() {
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};
    let srv = MockServer::start().await;
    Mock::given(method("POST")).and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"is_valid": true})))
        .mount(&srv).await;
    let v = HankoSessionValidator::new(srv.uri());
    assert!(matches!(v.validate("t").await, Err(AuthError::SessionRejected(_))));
}
```
- [ ] 2. Run `cargo test -p familiar-systems-app-shared`. Expect PASS.
- [ ] 3. Commit:
```
test(app-shared): cover validator error paths
```

---

### Task 6: Platform `Config` with env parsing

**Goal:** `Config::from_env()` reads `HANKO_API_URL` (required), `DATABASE_URL` (default `sqlite::memory:`), `PORT` (default `3000`), `CORS_ORIGINS` (required, comma-split).

**Files:**
- Create: `apps/platform/src/config.rs`
- Create: `apps/platform/src/lib.rs` (new — see Task 8 for rationale)
- Modify: `apps/platform/src/main.rs`

**Depends on:** Task 2

**Steps:**
- [ ] 1. Create `apps/platform/src/lib.rs`:
```rust
pub mod config;
```
- [ ] 2. Create `apps/platform/src/config.rs`:
```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub hanko_api_url: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let hanko_api_url = std::env::var("HANKO_API_URL")
            .expect("HANKO_API_URL is required. Set it in mise.toml [tasks.\"dev:platform\"].env or in the deployment manifest.");
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite::memory:".to_string());
        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        let cors_origins = std::env::var("CORS_ORIGINS")
            .expect("CORS_ORIGINS is required (comma-separated list of allowed origins)")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self { database_url, hanko_api_url, port, cors_origins }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn with_env<F: FnOnce()>(vars: &[(&str, &str)], f: F) {
        for (k, v) in vars { unsafe { std::env::set_var(k, v); } }
        f();
        for (k, _) in vars { unsafe { std::env::remove_var(k); } }
    }

    #[test]
    #[serial]
    fn parses_cors_origins_csv() {
        with_env(&[
            ("HANKO_API_URL", "https://x.hanko.io"),
            ("CORS_ORIGINS", "http://localhost:5173, https://app.familiar.systems"),
        ], || {
            let c = Config::from_env();
            assert_eq!(c.cors_origins, vec!["http://localhost:5173", "https://app.familiar.systems"]);
            assert_eq!(c.port, 3000);
            assert_eq!(c.database_url, "sqlite::memory:");
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "HANKO_API_URL is required")]
    fn panics_on_missing_hanko_url() {
        unsafe { std::env::remove_var("HANKO_API_URL"); }
        let _ = Config::from_env();
    }
}
```
- [ ] 3. Update `apps/platform/Cargo.toml` to add `[lib]` entry (implicit — `lib.rs` alongside `main.rs` auto-detects) and ensure the `[[bin]]` remains. If Cargo resolves both implicitly, no edit needed; verify with `cargo build -p familiar-systems-platform`.
- [ ] 4. Run `cargo test -p familiar-systems-platform config`. Expect PASS.
- [ ] 5. Commit:
```
feat(platform): add Config with env parsing
```

**Notes:** `cargo test` parallelises tests within a single process, so two tests that both touch `HANKO_API_URL` race on the shared env namespace. The `#[serial]` annotation from `serial_test` forces sequential execution of marked tests. Add `serial_test = "3"` to `apps/platform/Cargo.toml` under `[dev-dependencies]`.

---

### Task 7: Platform `AppError` with `IntoResponse`

**Goal:** Single error enum returning correct HTTP status codes.

**Files:**
- Create: `apps/platform/src/error.rs`
- Modify: `apps/platform/src/lib.rs`

**Depends on:** Task 2, Task 3

**Steps:**
- [ ] 1. Add `pub mod error;` to `apps/platform/src/lib.rs`.
- [ ] 2. Create `apps/platform/src/error.rs`:
```rust
use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use familiar_systems_app_shared::auth::AuthError;
use serde::Serialize;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("not found")]
    NotFound,
    #[error("internal: {0}")]
    Internal(String),
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Auth(#[from] AuthError),
}

#[derive(Serialize)]
struct ErrorBody { error: String }

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            AppError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),
            AppError::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")),
            AppError::Auth(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
        };
        (status, Json(ErrorBody { error: msg })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn unauthorized_maps_to_401() {
        let r = AppError::Unauthorized("nope".into()).into_response();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn auth_error_maps_to_401() {
        let r = AppError::Auth(AuthError::MissingHeader).into_response();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }
}
```
- [ ] 3. Run `cargo test -p familiar-systems-platform error`. Expect PASS.
- [ ] 4. Commit:
```
feat(platform): add AppError with IntoResponse
```

---

### Task 8: `AppState`, router, `/health`, `lib.rs`/`main.rs` split

**Goal:** `AppState { db, validator, config: Arc<Config> }`, `routes::router() -> Router<AppState>`, unauthenticated `GET /health`. Split binary from library so integration tests can import the router.

**Files:**
- Modify: `apps/platform/src/lib.rs`
- Create: `apps/platform/src/state.rs`
- Create: `apps/platform/src/routes/mod.rs`
- Create: `apps/platform/src/routes/health.rs`
- Modify: `apps/platform/src/main.rs`
- Create: `apps/platform/tests/health_test.rs`

**Depends on:** Task 6, Task 7

**Steps:**
- [ ] 1. Expand `apps/platform/src/lib.rs`:
```rust
pub mod config;
pub mod error;
pub mod routes;
pub mod state;
```
- [ ] 2. Create `apps/platform/src/state.rs`:
```rust
use crate::config::Config;
use familiar_systems_app_shared::auth::HankoSessionValidator;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub validator: Arc<HankoSessionValidator>,
    pub config: Arc<Config>,
}
```
- [ ] 3. Create `apps/platform/src/routes/health.rs`:
```rust
use axum::http::StatusCode;

pub async fn health() -> StatusCode {
    StatusCode::OK
}
```
- [ ] 4. Create `apps/platform/src/routes/mod.rs`:
```rust
mod health;
use crate::state::AppState;
use axum::{routing::get, Router};

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health::health))
}
```
- [ ] 5. Replace `apps/platform/src/main.rs` with a thin boot:
```rust
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{config::Config, routes::router, state::AppState};
use sea_orm::Database;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Arc::new(Config::from_env());
    let db = Database::connect(&config.database_url).await.expect("db connect");
    // Migrations wired in Task 12.
    let validator = Arc::new(HankoSessionValidator::new(config.hanko_api_url.clone()));
    let state = AppState { db, validator, config: config.clone() };
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await.unwrap();
    tracing::info!("platform listening on :{}", config.port);
    axum::serve(listener, router().with_state(state)).await.unwrap();
}
```
- [ ] 6. Create `apps/platform/tests/health_test.rs`:
```rust
use axum::{body::Body, http::{Request, StatusCode}};
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{config::Config, routes::router, state::AppState};
use sea_orm::Database;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_200() {
    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: "http://127.0.0.1:0".into(),
        port: 0,
        cors_origins: vec![],
    });
    let db = Database::connect(&config.database_url).await.unwrap();
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let state = AppState { db, validator, config };
    let app = router().with_state(state);
    let resp = app.oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
```
- [ ] 7. Run `cargo add --package familiar-systems-platform --dev tower --features util`.
- [ ] 8. Run `cargo test -p familiar-systems-platform --test health_test`. Expect PASS.
- [ ] 9. Commit:
```
feat(platform): router skeleton with /health
```

---

### Task 9: `users` SeaORM entity

**Goal:** Hand-written entity matching the `users` schema.

**Files:**
- Create: `apps/platform/src/entities/mod.rs`
- Create: `apps/platform/src/entities/users.rs`
- Modify: `apps/platform/src/lib.rs`

**Depends on:** Task 8

**Steps:**
- [ ] 1. Add `pub mod entities;` to `lib.rs`.
- [ ] 2. Create `entities/mod.rs`:
```rust
pub mod users;
```
- [ ] 3. Create `entities/users.rs`:
```rust
use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub hanko_sub: String,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveValue::Unchanged;

    #[test]
    fn model_into_active_model_roundtrip() {
        let now = Utc::now();
        let m = Model {
            id: Uuid::now_v7(),
            hanko_sub: "sub-1".into(),
            email: Some("a@b.com".into()),
            created_at: now,
            updated_at: now,
        };
        let am: ActiveModel = m.clone().into();
        // From<Model> for ActiveModel maps each field to ActiveValue::Unchanged
        // (preserving the loaded-from-db semantics). Use `Set(..)` only when
        // constructing an ActiveModel for an insert/update.
        assert_eq!(am.hanko_sub, Unchanged("sub-1".to_string()));
    }
}
```
- [ ] 4. Run `cargo test -p familiar-systems-platform entities::users`. Expect PASS.
- [ ] 5. Commit:
```
feat(platform): add users entity
```

---

### Task 10: Users migration

**Goal:** `m20260417_000001_create_users` migration producing the `users` table.

**Files:**
- Create: `apps/platform/src/migrations/mod.rs`
- Create: `apps/platform/src/migrations/m20260417_000001_create_users.rs`
- Modify: `apps/platform/src/lib.rs`

**Depends on:** Task 9

**Steps:**
- [ ] 1. Add `pub mod migrations;` to `lib.rs`.
- [ ] 2. Create `migrations/mod.rs`:
```rust
use sea_orm_migration::prelude::*;

mod m20260417_000001_create_users;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260417_000001_create_users::Migration)]
    }
}
```
- [ ] 3. Create `migrations/m20260417_000001_create_users.rs`:
```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    HankoSub,
    Email,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(
            Table::create()
                .table(Users::Table)
                .if_not_exists()
                .col(ColumnDef::new(Users::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(Users::HankoSub).string().not_null().unique_key())
                .col(ColumnDef::new(Users::Email).string().null())
                .col(ColumnDef::new(Users::CreatedAt).timestamp_with_time_zone().not_null())
                .col(ColumnDef::new(Users::UpdatedAt).timestamp_with_time_zone().not_null())
                .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Users::Table).to_owned()).await
    }
}
```
- [ ] 4. Run `cargo add --package familiar-systems-platform async-trait`.
- [ ] 5. Run `cargo check -p familiar-systems-platform`. Expect PASS.
- [ ] 6. Commit:
```
feat(platform): add initial users migration
```

---

### Task 11: Migration schema integration test

**Goal:** Running `Migrator::up()` against `sqlite::memory:` produces the expected columns.

**Files:**
- Create: `apps/platform/tests/migration_test.rs`

**Depends on:** Task 10

**Steps:**
- [ ] 1. Create `tests/migration_test.rs`:
```rust
use familiar_systems_platform::migrations::Migrator;
use sea_orm::{Database, ConnectionTrait, Statement};
use sea_orm_migration::MigratorTrait;

#[tokio::test]
async fn migrator_creates_users_table_with_hanko_sub_unique() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    let result = db.query_one(Statement::from_string(
        db.get_database_backend(),
        "select sql from sqlite_master where type='table' and name='users'".to_string(),
    )).await.unwrap().unwrap();
    let sql: String = result.try_get("", "sql").unwrap();
    assert!(sql.contains("hanko_sub"), "sql: {sql}");
    assert!(sql.to_lowercase().contains("unique"), "sql: {sql}");
}
```
- [ ] 2. Run `cargo test -p familiar-systems-platform --test migration_test`. Expect PASS.
- [ ] 3. Commit:
```
test(platform): migration creates users schema
```

---

### Task 12: Boot `Migrator::up` from `main`

**Goal:** `main.rs` runs migrations after DB connect. A smoke test confirms the boot path works end-to-end.

**Files:**
- Modify: `apps/platform/src/main.rs`
- Create: `apps/platform/tests/boot_test.rs`

**Depends on:** Task 11

**Steps:**
- [ ] 1. Edit `main.rs`, add after `Database::connect`:
```rust
use sea_orm_migration::MigratorTrait;
familiar_systems_platform::migrations::Migrator::up(&db, None).await.expect("migrate");
```
- [ ] 2. Create `tests/boot_test.rs` that starts the server on an ephemeral port in a `tokio::spawn`, hits `/health` via reqwest, asserts 200. Example:
```rust
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{config::Config, routes::router, state::AppState};
use sea_orm::Database;
use sea_orm_migration::MigratorTrait;
use std::sync::Arc;

#[tokio::test]
async fn boot_migrates_and_serves_health() {
    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: "http://127.0.0.1:0".into(),
        port: 0,
        cors_origins: vec![],
    });
    let db = Database::connect(&config.database_url).await.unwrap();
    familiar_systems_platform::migrations::Migrator::up(&db, None).await.unwrap();
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let state = AppState { db, validator, config };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router().with_state(state)).await.unwrap();
    });
    let body = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(body.status().as_u16(), 200);
}
```
- [ ] 3. Run `cargo test -p familiar-systems-platform --test boot_test`. Expect PASS.
- [ ] 4. Commit:
```
feat(platform): boot migrations then serve router
```

---

### Task 13: `AuthenticatedUser` extractor

**Goal:** Axum extractor that pulls `Authorization: Bearer`, validates via Hanko, upserts user, returns `AuthenticatedUser { id, hanko_sub, email }`.

**Files:**
- Create: `apps/platform/src/middleware/mod.rs`
- Create: `apps/platform/src/middleware/auth.rs`
- Modify: `apps/platform/src/lib.rs`

**Depends on:** Task 9, Task 12

**Steps:**
- [ ] 1. Add `pub mod middleware;` to `lib.rs`.
- [ ] 2. Create `middleware/mod.rs`:
```rust
pub mod auth;
```
- [ ] 3. Create `middleware/auth.rs`:
```rust
use crate::{error::AppError, state::AppState, entities::users};
use axum::{extract::FromRequestParts, http::request::Parts};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use sea_orm::sea_query::OnConflict;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub hanko_sub: String,
    pub email: Option<String>,
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let header = parts.headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::Unauthorized("missing authorization header".into()))?;
        let token = header.strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized("expected Bearer scheme".into()))?;
        let claims = app_state.validator.validate(token).await?;
        let email = claims.email.as_ref().map(|e| e.address.clone());
        let now = Utc::now();
        let am = users::ActiveModel {
            id: Set(Uuid::now_v7()),
            hanko_sub: Set(claims.subject.clone()),
            email: Set(email.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        users::Entity::insert(am)
            .on_conflict(
                OnConflict::column(users::Column::HankoSub)
                    .update_columns([users::Column::Email, users::Column::UpdatedAt])
                    .to_owned()
            )
            .exec(&app_state.db).await?;
        let row = users::Entity::find()
            .filter(users::Column::HankoSub.eq(&claims.subject))
            .one(&app_state.db).await?
            .ok_or(AppError::Internal("upsert did not land".into()))?;
        Ok(AuthenticatedUser { id: row.id, hanko_sub: row.hanko_sub, email: row.email })
    }
}
```
- [ ] 4. Run `cargo check -p familiar-systems-platform`. Expect PASS.
- [ ] 5. Commit:
```
feat(platform): AuthenticatedUser extractor with upsert
```

**Notes:** Docs checked: `sea-orm Insert::on_conflict` (stable in both 1.1 and 2.0 RC). `FromRef` is Axum's way to let a shared `AppState` be the source of the extractor state.

---

### Task 14: `spawn_app` test harness

**Goal:** Reusable `TestApp` that starts a wiremock `/sessions/validate` responder and a platform server on ephemeral ports.

**Files:**
- Create: `apps/platform/tests/common/mod.rs`
- Create: `apps/platform/tests/spawn_smoke.rs`

**Depends on:** Task 13

**Steps:**
- [ ] 1. Run `cargo add --package familiar-systems-platform --dev wiremock reqwest --features json`.
- [ ] 2. Create `tests/common/mod.rs`:
```rust
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{config::Config, routes::router, state::AppState};
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use std::sync::Arc;
use wiremock::MockServer;

pub struct TestApp {
    pub base_url: String,
    pub hanko: MockServer,
    pub db: DatabaseConnection,
}

pub async fn spawn_app() -> TestApp {
    let hanko = MockServer::start().await;
    let db = Database::connect("sqlite::memory:").await.unwrap();
    familiar_systems_platform::migrations::Migrator::up(&db, None).await.unwrap();
    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: hanko.uri(),
        port: 0,
        cors_origins: vec!["http://localhost:5173".into()],
    });
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let state = AppState { db: db.clone(), validator, config };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router().with_state(state)).await.unwrap();
    });
    TestApp { base_url: format!("http://{addr}"), hanko, db }
}
```
- [ ] 3. Create `tests/spawn_smoke.rs`:
```rust
mod common;

#[tokio::test]
async fn spawn_app_serves_health() {
    let app = common::spawn_app().await;
    let resp = reqwest::get(format!("{}/health", app.base_url)).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}
```
- [ ] 4. Run `cargo test -p familiar-systems-platform --test spawn_smoke`. Expect PASS.
- [ ] 5. Commit:
```
test(platform): add spawn_app + MockHankoServer harness
```

---

### Task 15: `/me` route + `no_token_returns_401`

**Goal:** Wire `GET /me` behind the extractor; cover missing-header case.

**Files:**
- Create: `apps/platform/src/routes/me.rs`
- Modify: `apps/platform/src/routes/mod.rs`
- Create: `apps/platform/tests/auth_test.rs`

**Depends on:** Task 14

**Steps:**
- [ ] 1. Create `routes/me.rs`:
```rust
use crate::middleware::auth::AuthenticatedUser;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct MeResponse {
    pub id: Uuid,
    pub hanko_sub: String,
    pub email: Option<String>,
}

pub async fn me(user: AuthenticatedUser) -> Json<MeResponse> {
    Json(MeResponse { id: user.id, hanko_sub: user.hanko_sub, email: user.email })
}
```
- [ ] 2. Update `routes/mod.rs` to register the route:
```rust
mod health;
mod me;
use crate::state::AppState;
use axum::{routing::get, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::health))
        .route("/me", get(me::me))
}
```
- [ ] 3. Create `tests/auth_test.rs`:
```rust
mod common;

#[tokio::test]
async fn no_token_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::get(format!("{}/me", app.base_url)).await.unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
```
- [ ] 4. Run `cargo test -p familiar-systems-platform --test auth_test`. Expect PASS.
- [ ] 5. Commit:
```
feat(platform): add /me route (401 on missing token)
```

---

### Task 16: `valid_token_returns_user_row`

**Files:** Modify `apps/platform/tests/auth_test.rs`

**Depends on:** Task 15

**Steps:**
- [ ] 1. Append test:
```rust
use sea_orm::{EntityTrait, ColumnTrait, QueryFilter};
use wiremock::{matchers::{method, path}, Mock, ResponseTemplate};

#[tokio::test]
async fn valid_token_returns_user_row_and_persists_it() {
    let app = common::spawn_app().await;
    Mock::given(method("POST")).and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": "test-sub",
                "email": {"address": "t@ex.com", "is_primary": true, "is_verified": true},
                "expiration": "2099-01-01T00:00:00Z",
                "session_id": "s"
            }
        })))
        .mount(&app.hanko).await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{}/me", app.base_url))
        .header("authorization", "Bearer fake")
        .send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["hanko_sub"], "test-sub");
    let count = familiar_systems_platform::entities::users::Entity::find()
        .filter(familiar_systems_platform::entities::users::Column::HankoSub.eq("test-sub"))
        .all(&app.db).await.unwrap().len();
    assert_eq!(count, 1);
}
```
- [ ] 2. Run `cargo test -p familiar-systems-platform --test auth_test valid_token`. Expect PASS.
- [ ] 3. Commit:
```
test(platform): valid token returns row and upserts
```

---

### Task 17: `invalid_token_returns_401` + `is_valid_false_returns_401`

**Files:** Modify `apps/platform/tests/auth_test.rs`

**Depends on:** Task 16

**Steps:**
- [ ] 1. Append two tests, one mounting a `ResponseTemplate::new(401)` responder, the other mounting `ResponseTemplate::new(200).set_body_json({"is_valid": false})`. Both call `/me` with `Authorization: Bearer x` and assert 401.
- [ ] 2. Run `cargo test -p familiar-systems-platform --test auth_test`. Expect PASS.
- [ ] 3. Commit:
```
test(platform): cover invalid and is_valid=false rejections
```

---

### Task 18: `upsert_is_idempotent`

**Files:** Modify `apps/platform/tests/auth_test.rs`

**Depends on:** Task 17

**Steps:**
- [ ] 1. Append test: mount `is_valid: true` responder, call `/me` twice, query DB for row by `hanko_sub`. Assert `count == 1` and `updated_at` on second query > first. Because SQLite may collapse identical timestamps, capture `updated_at` after first call, `tokio::time::sleep(Duration::from_millis(10)).await`, then call again.
- [ ] 2. Run `cargo test -p familiar-systems-platform --test auth_test upsert_is_idempotent`. If `updated_at` fails to advance, the `OnConflict::update_columns` in `middleware/auth.rs` is missing `UpdatedAt`; fix it. (Task 13 already includes `UpdatedAt`; this test locks it in.)
- [ ] 3. Commit:
```
test(platform): upsert is idempotent with advancing updated_at
```

---

### Task 19: CORS layer wired to `CORS_ORIGINS`

**Goal:** Apply `tower_http::cors::CorsLayer` with wildcard-aware origin predicate.

**Files:**
- Modify: `apps/platform/src/routes/mod.rs`
- Create: `apps/platform/tests/cors_test.rs`

**Depends on:** Task 15

**Steps:**
- [ ] 1. Build a `CorsLayer` at router construction time:
```rust
use tower_http::cors::{AllowOrigin, CorsLayer};
use axum::http::{HeaderValue, Method, HeaderName};

pub fn router(origins: Vec<String>) -> Router<AppState> {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([HeaderName::from_static("authorization"), HeaderName::from_static("content-type")])
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            let Ok(o) = origin.to_str() else { return false };
            origins.iter().any(|allowed| {
                if let Some(suffix) = allowed.strip_prefix("https://*.") {
                    o.strip_prefix("https://").map(|rest| rest.ends_with(&format!(".{suffix}")) || rest == suffix).unwrap_or(false)
                } else {
                    o == allowed
                }
            })
        }));
    Router::new()
        .route("/health", get(health::health))
        .route("/me", get(me::me))
        .layer(cors)
}
```
- [ ] 2. Update every caller of `router()` to pass `config.cors_origins.clone()`: `apps/platform/src/main.rs`, `apps/platform/tests/health_test.rs`, `apps/platform/tests/boot_test.rs`, and `apps/platform/tests/common/mod.rs`.
- [ ] 3. Create `tests/cors_test.rs` that sends a CORS preflight (`OPTIONS /me` with `Origin: http://localhost:5173`, `Access-Control-Request-Method: GET`) and asserts the response echoes the origin.
- [ ] 4. Run `cargo test -p familiar-systems-platform --test cors_test`. Expect PASS.
- [ ] 5. Commit:
```
feat(platform): CORS layer with wildcard preview-origin support
```

**Notes:** `CorsLayer::allow_origin(AllowOrigin::predicate(...))` is required because `AllowOrigin::list` does not handle `https://*.preview.familiar.systems`.

---

### Task 20: `mise.toml` `dev:platform` env block

**Goal:** Localhost dev works with a single `mise run dev:platform`.

**Files:** Modify `mise.toml`

**Depends on:** Task 12

**Steps:**
- [ ] 1. Locate `[tasks."dev:platform"]` in `mise.toml`. Add an `env` table:
```toml
[tasks."dev:platform"]
run = "cargo run -p familiar-systems-platform"
env = { HANKO_API_URL = "<dev-tenant-url>", DATABASE_URL = "sqlite://data/dev-platform.db?mode=rwc", PORT = "3000", CORS_ORIGINS = "http://localhost:5173", RUST_LOG = "info,familiar_systems_platform=debug" }
```
(Confirm exact table syntax matches neighbouring tasks in the file before committing; `mise.toml` may use inline tables or `[tasks."dev:platform".env]` sub-tables. Use whichever matches the existing style.)
- [ ] 2. Run `mkdir -p data && mise run dev:platform &` from repo root; wait 3s; curl `http://localhost:3000/health`; kill the background job. Expect `HTTP/1.1 200`.
- [ ] 3. Commit:
```
chore(mise): env block for dev:platform
```

**Notes:** `<dev-tenant-url>` placeholder — operator replaces with actual Hanko dev tenant URL (e.g., `https://f4*****.hanko.io`) before committing. Not a secret per brief §4.8.

---

### Task 21: Install `@teamhanko/hanko-elements` + JSX typing

**Goal:** Add the SDK and declare `<hanko-auth>` as a valid JSX element.

**Files:**
- Modify: `apps/web/package.json` (via pnpm)
- Create: `apps/web/src/custom.d.ts`

**Depends on:** none

**Steps:**
- [ ] 1. Run `pnpm --filter @familiar-systems/web add @teamhanko/hanko-elements`.
- [ ] 2. Create `apps/web/src/custom.d.ts`:
```ts
import type { DetailedHTMLProps, HTMLAttributes } from "react";

declare global {
  namespace JSX {
    interface IntrinsicElements {
      "hanko-auth": DetailedHTMLProps<HTMLAttributes<HTMLElement> & { api?: string }, HTMLElement>;
    }
  }
}

export {};
```
- [ ] 3. Run `mise run typecheck:ts`. Expect PASS.
- [ ] 4. Commit:
```
feat(web): add hanko-elements SDK and <hanko-auth> JSX types
```

---

### Task 22: Vite proxy + typed env vars

**Goal:** Change `/api` proxy target from `:3001` to `:3000`. Declare `VITE_HANKO_API_URL` and `VITE_API_BASE_URL` on `ImportMetaEnv`.

**Files:**
- Modify: `apps/web/vite.config.ts`
- Create: `apps/web/src/vite-env.d.ts`
- Create: `apps/web/.env.local.example`

**Depends on:** Task 21

**Steps:**
- [ ] 1. Edit `apps/web/vite.config.ts`. Change the `/api` proxy `target` from `http://localhost:3001` to `http://localhost:3000`. Leave `/collab` untouched (campaign server is unrelated to this slice).
- [ ] 2. Create `apps/web/src/vite-env.d.ts`:
```ts
/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_HANKO_API_URL: string;
  readonly VITE_API_BASE_URL?: string;
}
interface ImportMeta {
  readonly env: ImportMetaEnv;
}
```
- [ ] 3. Create `apps/web/.env.local.example`:
```
VITE_HANKO_API_URL=https://<dev-tenant>.hanko.io
# VITE_API_BASE_URL defaults to empty; Vite proxy forwards /api to localhost:3000 in dev.
```
- [ ] 4. Run `mise run typecheck:ts`. Expect PASS.
- [ ] 5. Commit:
```
feat(web): proxy /api to platform:3000, type Hanko env vars
```

---

### Task 23: `lib/hanko.ts` + `/login` route

**Goal:** Token-accessor helper plus a Login view that mounts `<hanko-auth>` and redirects on `sessionCreated`.

**Files:**
- Create: `apps/web/src/lib/hanko.ts`
- Create: `apps/web/src/login.tsx`
- Modify: `apps/web/src/App.tsx`

**Depends on:** Task 22

**Steps:**
- [ ] 1. Create `apps/web/src/lib/hanko.ts`:
```ts
import { Hanko } from "@teamhanko/hanko-elements";

export const hankoApiUrl = import.meta.env.VITE_HANKO_API_URL;
export const hanko = new Hanko(hankoApiUrl);

export function getSessionToken(): string | null {
  return hanko.getSessionToken();
}
```
- [ ] 2. Create `apps/web/src/login.tsx`:
```tsx
import { useEffect } from "react";
import { register } from "@teamhanko/hanko-elements";
import { hanko, hankoApiUrl } from "./lib/hanko";

export function Login() {
  useEffect(() => {
    register(hankoApiUrl).catch(console.error);
    const unsub = hanko.onSessionCreated(() => {
      window.location.assign("/");
    });
    return () => { unsub(); };
  }, []);
  return <hanko-auth api={hankoApiUrl} />;
}
```
- [ ] 3. Modify `apps/web/src/App.tsx`:
```tsx
import { Login } from "./login";
import { Home } from "./home";

export default function App() {
  if (window.location.pathname === "/login") return <Login />;
  return <Home />;
}
```
(Create a stub `home.tsx` for now that renders `<div>Loading…</div>`; replaced in Task 24.)
- [ ] 4. Run `mise run typecheck:ts && mise run lint:ts`. Expect PASS.
- [ ] 5. Commit:
```
feat(web): /login route with hanko-auth web component
```

**Notes:** `hanko.getSessionToken()` is the canonical accessor per `docs.hanko.io/resources/frontend-sdk` (returns the JWT or `null`). `onSessionCreated` returns an unsubscribe function.

---

### Task 24: Home route fetches `/api/me`

**Goal:** `/` fetches `/api/me` with Bearer token, renders JSON, redirects to `/login` on 401.

**Files:**
- Modify: `apps/web/src/home.tsx`

**Depends on:** Task 23

**Steps:**
- [ ] 1. Replace `home.tsx` with:
```tsx
import { useEffect, useState } from "react";
import { getSessionToken } from "./lib/hanko";

type Me = { id: string; hanko_sub: string; email: string | null };

export function Home() {
  const [me, setMe] = useState<Me | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const token = getSessionToken();
    if (!token) { window.location.assign("/login"); return; }
    fetch("/api/me", { headers: { Authorization: `Bearer ${token}` } })
      .then(async (r) => {
        if (r.status === 401) { window.location.assign("/login"); return; }
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        setMe(await r.json());
      })
      .catch((e) => setError(String(e)));
  }, []);
  if (error) return <pre>Error: {error}</pre>;
  if (!me) return <div>Loading...</div>;
  return <pre>{JSON.stringify(me, null, 2)}</pre>;
}
```
- [ ] 2. Run `mise run typecheck:ts && mise run lint:ts && mise run test:ts`. Expect PASS.
- [ ] 3. Run `mise run dev` in one terminal (or spawn `dev:platform` and `dev:web` in parallel); open `http://localhost:5173/login`; sign up with email + passcode; observe redirect to `/` and JSON render. Confirm via `sqlite3 data/dev-platform.db "select hanko_sub from users"` that the row landed.
- [ ] 4. Commit:
```
feat(web): home route renders /api/me
```

---

### Task 25: Platform Dockerfile

**Goal:** Multi-stage Rust build producing a minimal runtime image on port 3000.

**Files:**
- Create: `apps/platform/Dockerfile`
- Create: `apps/platform/.dockerignore`

**Depends on:** Task 12

**Steps:**
- [ ] 1. Create `apps/platform/.dockerignore`:
```
target
node_modules
.git
apps/web
apps/site
packages
workers
infra
docs
```
- [ ] 2. Create `apps/platform/Dockerfile`:
```dockerfile
# syntax=docker/dockerfile:1.7
FROM rust:1-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release -p familiar-systems-platform && \
    cp target/release/familiar-systems-platform /build/platform-bin

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /build/platform-bin /usr/local/bin/platform
EXPOSE 3000
ENV PORT=3000
USER nonroot
ENTRYPOINT ["/usr/local/bin/platform"]
```
- [ ] 3. Run `docker build -f apps/platform/Dockerfile -t platform:smoke .` from repo root. Expect success.
- [ ] 4. Run `docker run --rm -e HANKO_API_URL=http://example -e CORS_ORIGINS=http://localhost:5173 -e DATABASE_URL=sqlite::memory: -p 3000:3000 -d --name platform-smoke platform:smoke`; `sleep 2`; `curl -f http://localhost:3000/health`; `docker rm -f platform-smoke`. Expect `200 OK`.
- [ ] 5. Commit:
```
build(platform): multi-stage Rust Dockerfile
```

---

### Task 26: Web Dockerfile

**Goal:** Node build + nginx serve, accepting `VITE_HANKO_API_URL` and `VITE_API_BASE_URL` build args.

**Files:**
- Create: `apps/web/Dockerfile`
- Create: `apps/web/nginx.conf`
- Create: `apps/web/.dockerignore`

**Depends on:** Task 24

**Steps:**
- [ ] 1. Create `apps/web/nginx.conf`:
```
server {
  listen 80 default_server;
  root /usr/share/nginx/html;
  index index.html;
  location / {
    try_files $uri $uri/ /index.html;
  }
}
```
- [ ] 2. Create `apps/web/Dockerfile` modelled on `apps/site/Dockerfile`:
```dockerfile
# syntax=docker/dockerfile:1.7
FROM node:24-bookworm-slim AS builder
RUN corepack enable
WORKDIR /build
COPY . .
ARG VITE_HANKO_API_URL
ARG VITE_API_BASE_URL
ENV VITE_HANKO_API_URL=${VITE_HANKO_API_URL}
ENV VITE_API_BASE_URL=${VITE_API_BASE_URL}
RUN pnpm install --frozen-lockfile
RUN pnpm --filter @familiar-systems/web build

FROM nginx:1.27-alpine
COPY apps/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /build/apps/web/dist /usr/share/nginx/html
EXPOSE 80
```
- [ ] 3. Create `apps/web/.dockerignore` (exclude `node_modules`, `target`, `apps/platform/target`, etc.).
- [ ] 4. Run `docker build -f apps/web/Dockerfile --build-arg VITE_HANKO_API_URL=https://ex.hanko.io --build-arg VITE_API_BASE_URL=https://api-pr-1.preview.familiar.systems -t web:smoke .` from repo root. Expect success.
- [ ] 5. Commit:
```
build(web): multi-stage Dockerfile (node build + nginx serve)
```

---

### Task 27: Pulumi `config.py` constants

**Goal:** Add `api.familiar.systems` AND `app.familiar.systems` to `PRODUCTION_DOMAINS` (both, per user decision). Add `HANKO_API_URL_DEV` and `HANKO_API_URL_PROD` module constants.

**Files:** Modify `infra/pulumi-cloud/config.py`

**Depends on:** none

**Steps:**
- [ ] 1. Read `infra/pulumi-cloud/CLAUDE.md` for operator conventions (user runs `pulumi up`, not the agent).
- [ ] 2. Append `"api.familiar.systems"` and `"app.familiar.systems"` to `PRODUCTION_DOMAINS`.
- [ ] 3. Add module constants `HANKO_API_URL_DEV = "<dev-tenant-url>"` and `HANKO_API_URL_PROD = "<prod-tenant-url>"` (placeholders filled by operator before committing; not secrets per brief §4.8).
- [ ] 4. Run `cd infra/pulumi-cloud && uv run ruff check --fix . && uv run ruff format . && uv run basedpyright`. Expect PASS.
- [ ] 5. Commit:
```
infra(pulumi): add api + app familiar.systems SANs and Hanko URL constants
```

**Notes:** After this commit lands on a feature branch, prompt the user to run `pulumi preview` and report the diff. The wildcard cert re-issues with the new SANs; this should be the only notable change.

---

### Task 28: Pulumi `k8s.py` platform resources

**Goal:** Add production-tier platform PV, PVC, Deployment, Service, Ingress mirroring the existing `_site_*` pattern.

**Files:** Modify `infra/pulumi-cloud/k8s.py`

**Depends on:** Task 27

**Steps:**
- [ ] 1. Add module-level `PLATFORM_NAME = "platform"` and `PLATFORM_PORT = 3000`. Import `HANKO_API_URL_PROD`, `REGISTRY_PULL_SECRET`, `WILDCARD_CERT_SECRET` as already done for site resources.
- [ ] 2. Declare `_platform_pv` (`k8s.core.v1.PersistentVolume`, HostPath `/data/platform`, ReadWriteOnce, `persistentVolumeReclaimPolicy="Retain"`, `storageClassName=""`).
- [ ] 3. Declare `_platform_pvc` claiming `_platform_pv`.
- [ ] 4. Declare `_platform_deployment` mirroring `_site_deployment`: 1 replica, image `<registry>/platform:<tag>` (placeholder tag as in existing site deployment), volume mount `/data/platform` from PVC, env vars `HANKO_API_URL=HANKO_API_URL_PROD`, `DATABASE_URL=sqlite:///data/platform/platform.db?mode=rwc`, `CORS_ORIGINS="https://app.familiar.systems,https://familiar.systems"`, `PORT=3000`. `imagePullSecrets=[{"name": REGISTRY_PULL_SECRET}]`.
- [ ] 5. Declare `_platform_service` ClusterIP port 3000 → 3000.
- [ ] 6. Declare `_platform_ingress` routing `api.familiar.systems` to the service. TLS via `WILDCARD_CERT_SECRET`. Traefik `websecure` annotation as on `_site_ingress`.
- [ ] 7. Run `cd infra/pulumi-cloud && uv run ruff check --fix . && uv run ruff format . && uv run basedpyright`. Expect PASS.
- [ ] 8. Commit:
```
infra(pulumi): add platform PV, PVC, deployment, service, ingress
```
- [ ] 9. Tell the user the Pulumi change is ready; ask them to run `pulumi preview` and confirm no `+- replace` appears on existing resources.

**Notes:** Prod SPA deployment (`app.familiar.systems`) is deferred per brief §8.1. The SAN is added now so the cert re-issues once.

---

### Task 29: Rename existing preview manifests

**Goal:** `git mv` the three site manifests to `site-` prefix.

**Files:**
- `infra/k8s/preview/{deployment,service,ingress}.yaml` → `site-{deployment,service,ingress}.yaml`

**Depends on:** none

**Steps:**
- [ ] 1. Run:
```
git mv infra/k8s/preview/deployment.yaml infra/k8s/preview/site-deployment.yaml
git mv infra/k8s/preview/service.yaml    infra/k8s/preview/site-service.yaml
git mv infra/k8s/preview/ingress.yaml    infra/k8s/preview/site-ingress.yaml
```
- [ ] 2. Update the manifest-iteration loop in `.github/workflows/deploy-preview.yml` to reference the new filenames (the full workflow refactor is Task 35; this is a one-line keep-green fix).
- [ ] 3. Run `mise run lint:k8s && mise run lint:workflows`. Expect PASS.
- [ ] 4. Commit:
```
infra(preview): rename site manifests with site- prefix
```

**Notes:** `git mv` is required per your stored preference; preserves blame.

---

### Task 30: `platform-pv.yaml` + `platform-pvc.yaml`

**Goal:** Per-PR HostPath PV scoped to `/data/preview/pr-${PR_NUMBER}/platform`, bound to a per-PR PVC.

**Files:**
- Create: `infra/k8s/preview/platform-pv.yaml`
- Create: `infra/k8s/preview/platform-pvc.yaml`

**Depends on:** Task 29

**Steps:**
- [ ] 1. `platform-pv.yaml`:
```yaml
apiVersion: v1
kind: PersistentVolume
metadata:
  name: platform-pv-${NAMESPACE}
  labels:
    app: platform
    namespace: ${NAMESPACE}
spec:
  capacity:
    storage: 1Gi
  accessModes: [ReadWriteOnce]
  persistentVolumeReclaimPolicy: Retain
  storageClassName: ""
  claimRef:
    namespace: ${NAMESPACE}
    name: platform-pvc
  hostPath:
    path: /data/preview/${NAMESPACE}/platform
    type: DirectoryOrCreate
```
- [ ] 2. `platform-pvc.yaml`:
```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: platform-pvc
  namespace: ${NAMESPACE}
spec:
  accessModes: [ReadWriteOnce]
  storageClassName: ""
  volumeName: platform-pv-${NAMESPACE}
  resources:
    requests:
      storage: 1Gi
```
- [ ] 3. Validate: `NAMESPACE=preview-pr-1 envsubst < infra/k8s/preview/platform-pv.yaml | kubectl apply --dry-run=client -f -`. Expect no errors.
- [ ] 4. Run `mise run lint:k8s`. Expect PASS.
- [ ] 5. Commit:
```
infra(preview): platform HostPath PV and PVC
```

---

### Task 31: `platform-deployment.yaml` + service + ingress

**Files:**
- Create: `infra/k8s/preview/platform-deployment.yaml`
- Create: `infra/k8s/preview/platform-service.yaml`
- Create: `infra/k8s/preview/platform-ingress.yaml`

**Depends on:** Task 30

**Steps:**
- [ ] 1. `platform-deployment.yaml`: 1 replica, image `${PLATFORM_IMAGE}`, env `HANKO_API_URL=${HANKO_API_URL_DEV}`, `CORS_ORIGINS=https://${WEB_HOST}`, `DATABASE_URL=sqlite:///data/platform/platform.db?mode=rwc`, `PORT=3000`, `RUST_LOG=info`, volumeMount `/data/platform` from `platform-pvc`, `imagePullSecrets: [{ name: scaleway-registry }]`. Mirror resource requests/limits from existing `site` deployment.
- [ ] 2. `platform-service.yaml`: ClusterIP, port 3000 → 3000.
- [ ] 3. `platform-ingress.yaml`: Traefik `websecure` annotation, host `${PLATFORM_HOST}`, TLS `secretName: preview-wildcard-tls`.
- [ ] 4. Validate each manifest with `envsubst` + `kubectl apply --dry-run=client`. Expect no errors.
- [ ] 5. Run `mise run lint:k8s`. Expect PASS.
- [ ] 6. Commit:
```
infra(preview): platform deployment, service, ingress
```

---

### Task 32: `web-deployment.yaml` + service + ingress

**Files:**
- Create: `infra/k8s/preview/web-deployment.yaml`
- Create: `infra/k8s/preview/web-service.yaml`
- Create: `infra/k8s/preview/web-ingress.yaml`

**Depends on:** Task 31

**Steps:**
- [ ] 1. Model on the site manifests (post-rename). Deployment: image `${WEB_IMAGE}`, port 80, no env vars (SPA is prebuilt). Service ClusterIP 80. Ingress host `${WEB_HOST}`, TLS `preview-wildcard-tls`.
- [ ] 2. Validate with envsubst + `kubectl apply --dry-run=client`.
- [ ] 3. Run `mise run lint:k8s`. Expect PASS.
- [ ] 4. Commit:
```
infra(preview): web deployment, service, ingress
```

---

### Task 33: Workflow `paths:` filter

**Files:** Modify `.github/workflows/deploy-preview.yml`

**Depends on:** Task 32

**Steps:**
- [ ] 1. Extend `on.pull_request.paths` to include `apps/web/**`, `apps/platform/**`, `crates/app-shared/**`, `Cargo.toml`, `Cargo.lock`, `infra/k8s/preview/**`. Preserve existing entries.
- [ ] 2. Run `mise run lint:workflows`. Expect PASS.
- [ ] 3. Commit:
```
ci(preview): widen paths filter for web + platform + infra
```

---

### Task 34: Matrix build over `[site, web, platform]`

**Files:** Modify `.github/workflows/deploy-preview.yml`

**Depends on:** Task 25, Task 26, Task 33

**Steps:**
- [ ] 1. Convert the single build job to `strategy.matrix.target: [site, web, platform]` with `include:` entries:
```yaml
include:
  - target: site
    dockerfile: apps/site/Dockerfile
    image_name: site
    build_args: ""
  - target: web
    dockerfile: apps/web/Dockerfile
    image_name: web
    build_args: |
      VITE_HANKO_API_URL=${{ env.HANKO_API_URL_DEV }}
      VITE_API_BASE_URL=https://api-pr-${{ github.event.pull_request.number }}.preview.familiar.systems
  - target: platform
    dockerfile: apps/platform/Dockerfile
    image_name: platform
    build_args: ""
```
- [ ] 2. Expose each built image digest/tag via `outputs.image-${{ matrix.target }}` using `${{ steps.meta.outputs... }}` so the deploy job can reference them.
- [ ] 3. Add a workflow `env: HANKO_API_URL_DEV: https://<dev-tenant>.hanko.io` with a comment pointing at `infra/pulumi-cloud/config.py` as the canonical source (the workflow cannot `import` Python; keeping it synced is manual).
- [ ] 4. Run `mise run lint:workflows`. Expect PASS.
- [ ] 5. Commit:
```
ci(preview): matrix build for site, web, platform
```

**Notes:** The canonical source of `HANKO_API_URL_DEV` is Pulumi. Duplicating the URL in the workflow is acceptable because the value is public (not a secret). If keeping two sources in sync becomes painful, a later task can add a step that reads from Pulumi stack output.

---

### Task 35: Ordered manifest apply + rollout wait

**Files:** Modify `.github/workflows/deploy-preview.yml`

**Depends on:** Task 34

**Steps:**
- [ ] 1. In the deploy job, compute:
```
NAMESPACE=preview-pr-${PR_NUMBER}
WEB_HOST=app-pr-${PR_NUMBER}.preview.familiar.systems
PLATFORM_HOST=api-pr-${PR_NUMBER}.preview.familiar.systems
PR_HOST=pr-${PR_NUMBER}.preview.familiar.systems
SITE_IMAGE=<registry>/site:<matrix-site-tag>
WEB_IMAGE=<registry>/web:<matrix-web-tag>
PLATFORM_IMAGE=<registry>/platform:<matrix-platform-tag>
HANKO_API_URL_DEV=${{ env.HANKO_API_URL_DEV }}
```
(Export all of these for envsubst.)
- [ ] 2. Replace the apply loop:
```bash
for manifest in namespace registry-pull-secret \
                platform-pv platform-pvc \
                site-deployment site-service site-ingress \
                web-deployment web-service web-ingress \
                platform-deployment platform-service platform-ingress; do
  envsubst < "infra/k8s/preview/${manifest}.yaml" | kubectl apply -f -
done
```
- [ ] 3. Add rollout waits:
```bash
kubectl -n "$NAMESPACE" rollout status deployment/site --timeout=180s
kubectl -n "$NAMESPACE" rollout status deployment/web --timeout=180s
kubectl -n "$NAMESPACE" rollout status deployment/platform --timeout=180s
```
- [ ] 4. Run `mise run lint:workflows`. Expect PASS.
- [ ] 5. Commit:
```
ci(preview): apply platform + web + site manifests in order
```

---

### Task 36: PR comment with three URLs

**Files:** Modify `.github/workflows/deploy-preview.yml`

**Depends on:** Task 35

**Steps:**
- [ ] 1. Update the `actions/github-script` step template body to include three lines:
```
Site: https://${PR_HOST}
App:  https://${WEB_HOST}
API:  https://${PLATFORM_HOST}
```
- [ ] 2. Run `mise run lint:workflows`. Expect PASS.
- [ ] 3. Commit:
```
ci(preview): comment site, app, and api URLs on PR
```

---

### Task 37: Cleanup workflow updates

**Files:** Modify `.github/workflows/cleanup-preview.yml`

**Depends on:** Task 35

**Steps:**
- [ ] 1. Update the registry tag cleanup loop to iterate over `[site, web, platform]` image names.
- [ ] 2. Add `kubectl delete pv platform-pv-preview-pr-${PR_NUMBER} --ignore-not-found=true` (HostPath PVs with `Retain` policy do not auto-delete when the namespace is deleted; namespaced PVCs are cleaned by namespace delete).
- [ ] 3. Run `mise run lint:workflows`. Expect PASS.
- [ ] 4. Commit:
```
ci(preview): clean up platform PV and web/platform registry tags on PR close
```

---

## Verification

The slice is done when all of these pass without manual intervention.

### Local

- [ ] `mise run test` — all Vitest + cargo test + pytest suites green.
- [ ] `mise run typecheck` — tsc + cargo check + basedpyright green.
- [ ] `mise run lint` — oxlint + clippy + ruff + k8s + workflow lint green.
- [ ] `mise run format:check` — no formatting drift.
- [ ] `cargo test -p familiar-systems-platform` — 5 auth tests + migration test + health test + boot test + cors test all pass.
- [ ] Localhost round-trip (one terminal):
  ```
  mkdir -p data
  mise run dev           # starts site:4321, web:5173, platform:3000, campaign:3001
  # open http://localhost:5173/login in a browser
  # sign up with email + passcode on the dev Hanko tenant
  # observe redirect to / and JSON render of /api/me
  sqlite3 data/dev-platform.db 'select hanko_sub, email from users;'
  # expect: one row with the Hanko subject
  ```
- [ ] DevTools Network tab shows Hanko traffic going directly to `https://<dev-tenant>.hanko.io`; `/api/me` going through Vite proxy to `localhost:3000`.
- [ ] Direct Hanko sanity check:
  ```
  DEV_URL="https://<dev-tenant>.hanko.io"
  curl -sS "$DEV_URL/.well-known/config" | jq .   # 200 + config JSON
  curl -sS -X POST "$DEV_URL/sessions/validate" \
    -H "Content-Type: application/json" \
    -d '{"session_token":"not-a-real-token"}' \
    -o /tmp/body -w '%{http_code}\n'              # 4xx + JSON body
  ```

### Preview (per-PR)

- [ ] Open a PR against `main`. `deploy-preview.yml` runs: builds three images, applies manifests in order, comments three URLs.
- [ ] Navigate to `https://app-pr-N.preview.familiar.systems`. Hanko login works with passkeys (RP-ID matches `preview.familiar.systems`).
- [ ] CORS preflight from `https://app-pr-N...` to `https://api-pr-N...` returns 200 in DevTools.
- [ ] `GET /api/me` renders the user row cross-origin.
- [ ] Close PR: `cleanup-preview.yml` deletes namespace + PV + image tags.

### Pulumi (out-of-band, operator)

- [ ] After Task 28 lands, operator runs `pulumi preview`. Diff shows only additions and a cert re-issue with the two new SANs. No `+- replace` on existing resources.
- [ ] After review, operator runs `pulumi up`. `api.familiar.systems` resolves and `curl -sS -o /dev/null -w '%{http_code}' https://api.familiar.systems/health` returns 200 (once the image is published).

---

## Critical Files to Understand Before Starting

- `INITIAL_APP.md` — the design brief this plan implements.
- `CLAUDE.md` — dependency rules, mise commands, TypeScript strictness, TS/Rust/Python toolchain conventions.
- `infra/pulumi-cloud/CLAUDE.md` — operator-run Pulumi conventions.
- `docs/plans/2026-03-30-deployment-architecture.md` — preview hostname scheme (hyphens not dots).
- `infra/pulumi-cloud/k8s.py` — the raw `k8s.apps.v1.Deployment` pattern to mirror for platform resources.
- `apps/site/Dockerfile` — multi-stage pattern to model the web Dockerfile on.
- `crates/app-shared/src/id.rs` — the ts-rs `#[ts(export, export_to=...)]` convention.

---

## Out of Scope

- `apps/campaign` remains `fn main() {}`.
- `packages/types-campaign` sees no changes (Loro CRDT, ToC schema, PM conventions all untouched).
- `workers/` — not touched.
- Production SPA deployment at `app.familiar.systems` — SAN is added, deployment is a follow-up.
- Campaign CRUD, routing table, shard registry, billing, suggestions — follow-up plans.
- Observability beyond `tracing_subscriber` with `EnvFilter` — follow-up plan.
- Token refresh / explicit `sessionExpired` handling on the client beyond redirect-to-login on 401.
- Response-body sanitization of internal errors. `AppError::Db(e)` currently renders `format!("db: {e}")` into the HTTP body, which exposes `sea_orm::DbErr` Display content (constraint names, column names, SQL fragments). `AppError::Auth(_)` surfaces the inner `AuthError::RequestFailed(reqwest::Error)` Display, which may include the Hanko tenant URL. Hardening pass: log full error at `tracing::error!`, return a generic string to clients. Track for a dedicated follow-up plan before prod traffic arrives.

---

## Execution Notes

Tasks 3–18 (Rust backend) and Tasks 21–26 (frontend + Dockerfiles) share no source files and can be implemented in separate worktrees concurrently when using subagent-driven mode. Tasks 27–37 (infra + CI) depend on both Dockerfiles existing but not on the feature tests passing, so the infra track can start once Tasks 25 and 26 land.
