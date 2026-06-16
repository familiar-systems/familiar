//! Document-domain wire types: requests/responses for Pages and their blocks.
//!
//! Visible to the FE (the SPA creates Pages and reads them back), so the
//! structs live here and are exported via ts-rs into `packages/types-campaign`
//! under `generated/document/`.

pub mod pages;
pub mod sessions;
