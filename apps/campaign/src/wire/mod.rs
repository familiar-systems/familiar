//! Wire-protocol utilities for the loro-protocol websocket layer.
//!
//! These utilities live outside `domain/crdt/` because they are wire concerns,
//! not CRDT semantics. `CrdtRoom` operates on already-assembled
//! `Vec<Vec<u8>>` updates and never sees fragmentation, batch IDs, or
//! reassembly state. The actor that wraps a room composes the utilities in
//! this module to adapt the wire protocol to the room's pure interface.
//!
//! ## Submodules
//!
//! - [`assembler`] — `BatchAssembler`, the pure state machine that
//!   reassembles inbound `DocUpdateFragmentHeader + N · DocUpdateFragment`
//!   sequences into a single CRDT update payload.
//! - [`fragmenter`] — `BatchFragmenter`, the symmetric outbound splitter
//!   for broadcasts that exceed the protocol's 256 KB per-message cap.
//! - [`reassembly`] — kameo-side wiring: `FragmentTimeout` self-message
//!   and `schedule_fragment_timeout` helper that enforce the protocol's
//!   10-second reassembly timeout via `tokio::spawn` + `tokio::time::sleep`.
//!
//! ## References
//!
//! - Wire format: [loro-protocol v0.3.0 protocol.md](https://github.com/loro-dev/protocol/blob/loro-protocol-v0.3.0/protocol.md).
//! - End-to-end validation: the
//!   [`tiptap-loro-kameo-rust`](../../../../experiment-single-campaign-editor/tiptap-loro-kameo-rust)
//!   spike.
//! - Reference server impl:
//!   [`loro-websocket-server`](https://github.com/loro-dev/protocol/blob/loro-protocol-v0.3.0/rust/loro-websocket-server/src/lib.rs).
//!   Our keying strategy is the same shape; we additionally enforce the
//!   reassembly timeout the reference impl leaves unimplemented.

pub mod assembler;
pub mod fragmenter;
pub mod reassembly;
