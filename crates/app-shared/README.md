# familiar-systems-app-shared

Application-level shared types used by both the platform and campaign servers.

Types here cross the platform/campaign boundary: some IDs, auth primitives.

The litmus test: **does the platform server need this type?** If yes, it belongs here. If only the campaign server uses it, it belongs in `campaign-shared`.

ID types are defined with the `#[fs_id]` macro (from the `fs-id` utility crate) and exported to `packages/types-app/` via ts-rs alongside other `#[derive(TS)]` types.
