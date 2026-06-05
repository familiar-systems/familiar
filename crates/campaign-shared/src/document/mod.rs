//! Document-domain wire types: requests/responses for Things and their blocks.
//!
//! Visible to the FE (the SPA creates Things and reads them back), so the
//! structs live here and are exported via ts-rs into `packages/types-campaign`
//! under `generated/document/`.

pub mod things;
