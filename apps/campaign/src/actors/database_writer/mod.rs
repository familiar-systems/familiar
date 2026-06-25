//! `DatabaseWriteActor`: single owner of the per-campaign sea-orm write connection.
//!
//! Facade module. The actor and most write handlers live in
//! `database_writer_actor`; the relationship write primitives in
//! `database_writer_relationships`. Re-exports preserve the
//! `crate::actors::database_writer::*` import surface.

mod database_writer_actor;
mod database_writer_relationships;
#[cfg(test)]
mod tests;

pub use database_writer_actor::*;
pub use database_writer_relationships::*;
