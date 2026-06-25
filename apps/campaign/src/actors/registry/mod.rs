//! `CampaignRegistry`: process-lifetime owner of per-campaign supervisors.
//!
//! Facade module. The implementation lives in `registry_actor`; these
//! re-exports preserve the `crate::actors::registry::*` import surface.

mod registry_actor;
#[cfg(test)]
mod tests;

pub use registry_actor::*;
