//! Loro-backed concrete implementations of `CrdtDoc`.
//!
//! Each submodule wraps a `loro::LoroDoc` for a specific document type
//! (Thing pages, Table of Contents). Schema constants and ts-rs-exported
//! domain types live in `familiar_systems_campaign_shared::loro`; the
//! wrappers below are Rust-only and consumed solely by the campaign server.

pub mod thing;
pub mod toc;
