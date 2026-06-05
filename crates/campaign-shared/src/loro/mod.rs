//! Loro doc schema: ts-rs-exported domain types and ProseMirror conventions.
//!
//! This module holds *types* and *constants* that the Rust and TypeScript
//! sides must agree on for Loro doc layout. Concrete `LoroDoc` wrappers and
//! the `CrdtDoc` trait live in `apps/campaign/src/loro/` because they have
//! no cross-crate or cross-language consumers.

pub mod page;
pub mod prosemirror;
pub mod toc;
