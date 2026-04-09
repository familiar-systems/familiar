# familiar-systems-app-shared

Application-level shared types used by both the platform and campaign servers.

Types here cross the platform/campaign boundary: IDs, ThingHandle, Status, auth primitives, libSQL helpers.

The litmus test: **does the platform server need this type?** If yes, it belongs here. If only the campaign server uses it, it belongs in `campaign-shared`.

All types with `#[derive(TS)]` export to `packages/types-app/` via ts-rs.
