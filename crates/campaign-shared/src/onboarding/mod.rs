//! Onboarding wire types: catalog response + initialize request.
//!
//! Both ends are visible to the FE (the SPA reads the catalog and posts the
//! initialize payload), so the structs live here and are exported via ts-rs
//! into `packages/types-campaign`. Locale resolution happens on the campaign
//! side before responses leave; the FE never sees `LocalizedString`.

pub mod catalog;
pub mod initialize;
