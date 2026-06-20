---
name: loro-crdt
description: >-
  Expert guidance for the Loro CRDT library and loro-protocol sync
  protocol in Rust and TypeScript. Covers LoroDoc, container selection
  (Map, List, Text, MovableList, Tree, Counter), WebSocket sync via
  loro-protocol (handshake, rooms, CrdtDocAdaptor trait), encoding/export,
  undo/redo, event subscriptions, time travel, cursors, and ephemeral state.
  MUST invoke before any response that reads or modifies files under
  `*/loro/*`, `*/loro*.rs`, or `*/crdt/*`, or when the user's message
  or referenced files contain Loro types (LoroDoc, LoroMap, LoroTree,
  etc.). Also invoke when working with collaborative editing,
  real-time sync, WebSocket sync, or CRDTs.
  Trigger: "loro", "crdt", "collaborative editing", "loro-crdt",
  "loro-protocol", "LoroDoc", "LoroMap", "LoroText", "LoroList",
  "LoroTree", "CrdtDocAdaptor", "WebSocket sync".
---

# Loro CRDT

Loro is a high-performance CRDT framework. It is a pure library: it handles conflict-free data structures and merge semantics, but not networking, storage, or transport. You export bytes and import bytes; everything in between is your responsibility.

Rust-native (`loro` crate), TypeScript via WASM (`loro-crdt` npm package). `LoroDoc` is the entry point for both.

For comprehensive API details beyond this skill, fetch the LLM-optimized reference:
**https://loro.dev/llms-full.txt** (642KB, the entire docs site in one file)

## Documentation Links

### Getting Started & Core Concepts

- [Get Started](https://loro.dev/docs/tutorial/get_started) -- install, bundler setup, first sync
- [Core Concepts](https://loro.dev/docs/tutorial/concepts) -- attached/detached, OpLog vs DocState, version vectors, frontiers
- [LoroDoc](https://loro.dev/docs/tutorial/loro_doc) -- the main entry point, container access, commits, forking

### Container Tutorials (both Rust and TypeScript examples)

- [Map](https://loro.dev/docs/tutorial/map) -- LWW key-value, nested containers, conflict resolution
- [List](https://loro.dev/docs/tutorial/list) -- ordered sequence, Fugue algorithm
- [Text / Richtext](https://loro.dev/docs/tutorial/text) -- rich text with marks, ExpandType config
- [MovableList](https://loro.dev/docs/tutorial/movable_list) -- list with CRDT-safe reordering
- [Tree](https://loro.dev/docs/tutorial/tree) -- hierarchical movable tree, fractional index siblings
- [Counter](https://loro.dev/docs/tutorial/counter) -- additive distributed counter

### Collaboration & Versioning

- [Version Control](https://loro.dev/docs/tutorial/version) -- VersionVector, Frontiers, diffing
- [Encoding & Export](https://loro.dev/docs/tutorial/encoding) -- snapshot, updates, shallow snapshot, state-only
- [Events](https://loro.dev/docs/tutorial/event) -- subscription model, event types per container
- [Awareness / Ephemeral](https://loro.dev/docs/tutorial/awareness) -- presence, cursor sharing, EphemeralStore

### Advanced

- [Time Travel](https://loro.dev/docs/tutorial/time_travel) -- checkout, revert, detached editing
- [Cursor](https://loro.dev/docs/tutorial/cursor) -- stable positions that survive concurrent edits
- [Undo Manager](https://loro.dev/docs/advanced/undo) -- per-peer undo/redo, grouping, cursor restoration
- [Shallow Snapshots](https://loro.dev/docs/advanced/shallow_snapshot) -- trimmed history for storage/privacy

### Sync Protocol (loro-protocol)

The `loro-protocol` library provides a complete WebSocket sync protocol on top of Loro's raw `export`/`import` primitives: handshake, room management, fragmentation, and an adaptor trait.

- [Blog post (design rationale)](https://loro.dev/blog/loro-protocol)
- [GitHub repo](https://github.com/loro-dev/protocol)
- [Wire spec (protocol.md)](https://github.com/loro-dev/protocol/blob/main/protocol.md)
- [LLM reference (llms.md)](https://github.com/loro-dev/protocol/blob/main/llms.md)
- [E2EE extension spec](https://github.com/loro-dev/protocol/blob/main/protocol-e2ee.md)

### API References

**Rust** (docs.rs, authoritative for Rust API):
- [loro crate root](https://docs.rs/loro/latest/loro/)
- [LoroDoc](https://docs.rs/loro/latest/loro/struct.LoroDoc.html)
- [LoroMap](https://docs.rs/loro/latest/loro/struct.LoroMap.html) | [LoroList](https://docs.rs/loro/latest/loro/struct.LoroList.html) | [LoroText](https://docs.rs/loro/latest/loro/struct.LoroText.html)
- [LoroTree](https://docs.rs/loro/latest/loro/struct.LoroTree.html) | [LoroMovableList](https://docs.rs/loro/latest/loro/struct.LoroMovableList.html) | [LoroCounter](https://docs.rs/loro/latest/loro/struct.LoroCounter.html)
- [ExportMode](https://docs.rs/loro/latest/loro/enum.ExportMode.html) | [LoroValue](https://docs.rs/loro/latest/loro/enum.LoroValue.html)
- [UndoManager](https://docs.rs/loro/latest/loro/struct.UndoManager.html)

**TypeScript**:
- [JS API Reference](https://loro.dev/docs/api/js) -- method-level docs with examples

### Language-Specific Patterns (reference files in this skill)

- Rust idioms, cargo setup, trait system: read `references/rust-patterns.md`
- TypeScript idioms, WASM bundler setup: read `references/typescript-patterns.md`
- Sync protocols, export modes, versioning deep-dive: read `references/sync-and-encoding.md`
- WebSocket sync protocol, CrdtDocAdaptor trait, handshake, rooms: read `references/loro-protocol.md`

## Container Selection

What kind of data are you modeling?

**Key-value pairs, properties, settings, or labeled fields?**
Use `LoroMap`. LWW conflict resolution (Lamport timestamp; higher PeerID wins ties).
Do NOT use a list of pairs; do NOT use a map for ordered sequences.
[Tutorial](https://loro.dev/docs/tutorial/map) | [Rust API](https://docs.rs/loro/latest/loro/struct.LoroMap.html)

**Ordered collection of items?**
- Items need drag-and-drop / reordering? Use `LoroMovableList`. ~80% slower encode, ~50% more memory than LoroList. [Tutorial](https://loro.dev/docs/tutorial/movable_list)
- Append-only or no reordering? Use `LoroList`. Fugue algorithm for maximal non-interleaving. [Tutorial](https://loro.dev/docs/tutorial/list)

**Editable text (plain or rich)?**
Use `LoroText`. O(log N) insert/delete. Concurrent edits merge by interleaving characters.
Configure mark expansion with `config_text_style` before inserting marks.
[Tutorial](https://loro.dev/docs/tutorial/text) | [Rust API](https://docs.rs/loro/latest/loro/struct.LoroText.html)

**Hierarchical / tree-shaped data (folders, outlines, org charts)?**
Use `LoroTree`. Based on Kleppmann's algorithm. Enable `fractional_index` for ordered siblings.
[Tutorial](https://loro.dev/docs/tutorial/tree) | [Rust API](https://docs.rs/loro/latest/loro/struct.LoroTree.html)

**Numeric accumulator (votes, counters, scores)?**
Use `LoroCounter`. Additive CRDT. Requires `features = ["counter"]` in Rust.
[Tutorial](https://loro.dev/docs/tutorial/counter)

**A string value where concurrent edits should pick a winner, not merge characters?**
Use a `String` value inside `LoroMap` (LWW), NOT `LoroText`.
LoroText merges concurrent insertions character-by-character. Map picks one winner.
Good for: URLs, identifiers, hashes, enum-like values.

## LoroDoc Lifecycle

```rust
// Rust
let doc = LoroDoc::new();
let map = doc.get_map("config");
map.insert("key", "value").unwrap();
doc.commit();

let bytes = doc.export(ExportMode::Snapshot).unwrap();
let restored = LoroDoc::from_snapshot(&bytes).unwrap();
let forked = doc.fork(); // deep copy (NOT clone(), which is a reference)
```

```typescript
// TypeScript
import { LoroDoc } from "loro-crdt";
const doc = new LoroDoc();
const map = doc.getMap("config");
map.set("key", "value");
doc.commit();

const bytes = doc.export({ mode: "snapshot" });
const restored = LoroDoc.fromSnapshot(bytes);
const forked = doc.fork(); // deep copy
```

Full lifecycle tutorial: [LoroDoc](https://loro.dev/docs/tutorial/loro_doc)

## Critical Pitfalls

**CRITICAL: Never reuse PeerID across concurrent writers.**
Each writer must have a globally unique PeerID. Reusing one across concurrent sessions permanently corrupts the document by producing conflicting operation IDs in the OpLog DAG. There is no recovery. Default behavior (random assignment) is safe; only call `setPeerId()` / `set_peer_id()` when you have a durable, unique identifier per writer.

**CRITICAL: `clone()` is a reference clone in Rust.**
`doc.clone()` gives you a second handle to the same underlying document. Mutations through either handle affect both. Use `doc.fork()` for an independent deep copy. In TypeScript, always use `doc.fork()`.

**HIGH: Concurrent sub-container creation at the same map key.**
If two peers both `insert_container("child", LoroText::new())` on the same map key, LWW picks one container and the other's edits are silently lost. Initialize all child containers during setup, or use distinct keys / root-level containers.

**HIGH: Document is read-only after `checkout()`.**
Calling `checkout(frontiers)` enters detached mode. Edits will fail unless you call `set_detached_editing(true)` first. Call `attach()` or `checkout_to_latest()` / `checkoutToLatest()` to resume normal editing.

**HIGH: Timestamps are not recorded by default.**
Call `doc.set_record_timestamp(true)` / `doc.setRecordTimestamp(true)` before making edits. This is a runtime setting, not persisted in the document.

**MEDIUM: LoroText vs LoroMap for string values.**
LoroText merges concurrent edits character-by-character (correct for prose). For values where partial merge breaks semantics (URLs, IDs, config strings), store as a `String` in `LoroMap` so LWW picks one winner.

**MEDIUM: Text indexing differs between Rust and TypeScript.**
TypeScript uses UTF-16 offsets by default. Rust uses Unicode scalar positions. When sharing cursor positions or text ranges cross-language, use the explicit `_utf8` / `_utf16` method variants and convert deliberately.

## Rust vs TypeScript Quick Reference

| Concern | Rust | TypeScript |
|---|---|---|
| Package | `loro` crate | `loro-crdt` npm |
| Entry point | `LoroDoc::new()` | `new LoroDoc()` |
| Text index unit | Unicode scalar | UTF-16 |
| PeerID type | `u64` | `bigint` / decimal string |
| Unsubscribe | Drop the `Subscription` object | Call returned function |
| Ephemeral state | `awareness` module | `EphemeralStore` |
| Deep copy | `fork()` (NOT `clone()`) | `fork()` |
| Counter feature | `features = ["counter"]` | Built-in |
| Events | Synchronous | Synchronous |
| Map set method | `map.insert(key, value)` | `map.set(key, value)` |
| Sub-containers | `map.insert_container(key, T::new())` | `map.setContainer(key, new T())` |

## Sync Protocol

Two-peer sync at the primitive level requires two exchanges:

```rust
// Peer A sends updates Peer B hasn't seen
let updates = doc_a.export(ExportMode::updates(&doc_b.oplog_vv())).unwrap();
doc_b.import(&updates).unwrap();
// Peer B sends updates Peer A hasn't seen
let updates = doc_b.export(ExportMode::updates(&doc_a.oplog_vv())).unwrap();
doc_a.import(&updates).unwrap();
```

For real-time streaming, use `subscribe_local_update` / `subscribeLocalUpdates` to push bytes as they are produced.

For encoding modes, export strategies, and shallow snapshots, read `references/sync-and-encoding.md`.

**For WebSocket-based sync**, the `loro-protocol` library provides a complete protocol on top of these primitives: handshake with auth, room management, automatic fragmentation, error handling, and an adaptor trait (`CrdtDocAdaptor`) for plugging in your LoroDoc. Read `references/loro-protocol.md` for the full API.
