# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

Covers `crates/` only: the four Rust library crates the two servers share. Overrides the repo-root CLAUDE.md within this directory. Per-app wiring (entities, ORM, actors) stays in `apps/*` — see those crates' docs.

| Crate | Purpose | Feeds TS package |
| --- | --- | --- |
| `fs-id-macros` | `#[fs_id]` proc-macro (branded-ID codegen) | — |
| `fs-id` | re-exports the macro + allowed inner types (`Nanoid`, `Uuid`, `Ulid`) | — |
| `app-shared` | types both servers need: IDs (`CampaignId`, `UserId`), auth (Hanko validator, `AuthenticatedUser` extractor), campaign-CRUD DTOs | `types-app` |
| `campaign-shared` | campaign-only types: IDs (`PageId`, `BlockId`, ...), Loro schema constants, onboarding/notification DTOs | `types-campaign` |

Split rule (from root CLAUDE.md): "does the platform server need this type?" Yes → `app-shared`. No → `campaign-shared`.

## Branded IDs: `#[fs_id]`

Define a type-safe newtype ID by wrapping an inner type from the allowlist — `Nanoid`/`Uuid`/`Ulid` are generative (get `generate()`); `u64`/`u32`/`i64`/`i32` are value-only:

```rust
// crates/app-shared/src/id.rs
#[fs_id(export_to = "types-app/src/generated/id/")]
pub struct CampaignId(pub Nanoid);
```

The macro emits serde, `TS`, `ToSchema`, `Display`, `From<Inner>`, a const `new(value)`, and `generate()` (generative inners only). The TS brand is locked to the struct name — `string & { __brand: "CampaignId" }` — so a rename can't silently drift. `export_to` is the only knob; omit it and the type isn't written to any package.

## Type generation: Rust → TypeScript

`mise run generate-types` is the single command that turns Rust types into the `@familiar-systems/types-{app,campaign}` packages. **Run it after changing any `#[derive(TS)]` / `#[fs_id]` type or any `utoipa` route.** The pipeline (`.mise/tasks/generate-types`):

1. `rm -rf packages/types-*/src/generated` — clears ghosts left by renames/deletes.
2. `cargo test --workspace` — ts-rs writes each `#[ts(export, export_to = ...)]` type to its `.ts` file **as a test side effect** (this is why codegen rides on the test run, not a dedicated binary). Base dir `packages/` comes from `TS_RS_EXPORT_DIR` in `.cargo/config.toml`; `export_to` is appended and is **workspace-root-relative** (`"types-app/src/generated/id/"`, never `../../packages/...`).
3. Writes a barrel `index.ts` (`export *`) into each `generated/*/` subdir.
4. `emit-openapi` / `emit-openapi-campaign` binaries dump the utoipa specs to JSON; `tooling/openapi-codegen` turns them into a typed `paths` interface whose component schemas import back the ts-rs branded types.
5. `oxfmt` formats the output.

Committed vs. generated:

- **Hand-written, committed:** each package's `src/index.ts` (its public API) and `src/schemas.ts` (runtime zod guards — ts-rs emits compile-time types only).
- **Generated, committed, never hand-edit:** everything under `src/generated/` and `src/openapi/`, plus `openapi/*.json`. Checked in so TS consumers don't run codegen — only crate authors do. Hand edits are pointless (overwritten next run).

## Adding a shared type

1. Place it in `app-shared` or `campaign-shared` per the split rule.
2. Derive `#[derive(Serialize, TS, ToSchema)]` (+ `Deserialize` if it's an input) with `#[ts(export, export_to = "types-{app,campaign}/src/generated/<area>/")]`. For an ID, use `#[fs_id(export_to = ...)]` instead.
3. `mise run generate-types`, then re-export it from the package's hand-written `src/index.ts`.
4. If it arrives as an unverified string (URL param, untyped JSON), add a zod guard in `src/schemas.ts`.
