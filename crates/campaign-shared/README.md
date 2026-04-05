# loreweaver-campaign-shared

Campaign-scoped shared library. Schema types, Loro document wrappers, and the CrdtDoc trait.

Everything here is used exclusively by the campaign server. The platform server does not depend on this crate.

Depends on `app-shared` for IDs and `loro` for CRDT operations.

All types with `#[derive(TS)]` export to `packages/types/` via ts-rs.
