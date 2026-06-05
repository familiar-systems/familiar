# Loro Sync & Encoding Patterns

## Mental Model

A `LoroDoc` has two internal components:

1. **OpLog**: append-only DAG of all operations ever applied. Immutable history.
2. **DocState**: the current materialized state, computed from OpLog.

These can be in sync ("attached") or out of sync ("detached", after a `checkout`).

Sync is the process of exchanging OpLog entries between peers so their histories converge. Once two peers have the same OpLog, their DocStates are identical.

Docs: [Core Concepts](https://loro.dev/docs/tutorial/concepts)

## Version Representations

**VersionVector**: `Map<PeerId, Counter>`. Tracks the latest known counter for each peer. Used for "what have you seen?" queries. Grows linearly with peer count.

**Frontiers**: the "tips" of the causal DAG. Usually a single operation ID when edits are sequential. More compact than a VersionVector for most operations.

Use **VersionVector** for sync protocol (computing deltas). Use **Frontiers** for checkout, fork, and shallow snapshot boundaries.

```rust
// Rust
let oplog_vv: VersionVector = doc.oplog_vv();
let state_vv: VersionVector = doc.state_vv();
let frontiers: Frontiers = doc.state_frontiers();
let oplog_f: Frontiers = doc.oplog_frontiers();

// Convert between representations
let vv = doc.frontiers_to_vv(&frontiers);
let f = doc.vv_to_frontiers(&vv);
```

```typescript
// TypeScript
const oplogVV = doc.oplogVersion();
const stateVV = doc.version();
const frontiers = doc.frontiers();
const oplogF = doc.oplogFrontiers();
```

Docs: [Version tutorial](https://loro.dev/docs/tutorial/version)

## Two-Peer Sync Protocol

The basic sync protocol requires two exchanges. Each peer tells the other "here is everything you haven't seen":

```rust
// Rust
// Step 1: A sends updates B hasn't seen
let updates_a = doc_a.export(ExportMode::updates(&doc_b.oplog_vv())).unwrap();
doc_b.import(&updates_a).unwrap();

// Step 2: B sends updates A hasn't seen
let updates_b = doc_b.export(ExportMode::updates(&doc_a.oplog_vv())).unwrap();
doc_a.import(&updates_b).unwrap();
```

```typescript
// TypeScript
const updatesA = docA.export({ mode: "update", from: docB.oplogVersion() });
docB.import(updatesA);

const updatesB = docB.export({ mode: "update", from: docA.oplogVersion() });
docA.import(updatesB);
```

In practice, peers exchange version vectors first, then send only the delta.

## Real-Time Streaming

> For a complete WebSocket sync protocol with handshake, room management, auth, and error handling, see `references/loro-protocol.md`. The patterns below show the raw primitives that the protocol builds on.

For WebSocket or similar real-time channels, subscribe to local updates and broadcast them:

```rust
// Rust
doc.subscribe_local_update(Box::new(move |bytes| {
    websocket_send(bytes);
    true // return false to auto-unsubscribe
}));

// On receive:
doc.import(&received_bytes).unwrap();
```

```typescript
// TypeScript
doc.subscribeLocalUpdates((bytes: Uint8Array) => {
    websocket.send(bytes);
});

// On receive:
doc.import(receivedBytes);
```

## Batch Import

When importing multiple update messages at once, use batch import for efficiency. It computes the diff once instead of per-message:

```rust
// Rust
doc.import_batch(&[bytes1, bytes2, bytes3]).unwrap();
```

```typescript
// TypeScript
doc.importBatch([bytes1, bytes2, bytes3]);
```

## ImportStatus

Import returns status indicating what was successfully applied and what is pending (missing causal dependencies):

```rust
// Rust
let status = doc.import(&bytes).unwrap();
// status.success: ranges applied
// status.pending: ranges waiting for missing dependencies
```

Pending updates are held internally and auto-applied when their dependencies arrive.

## Export Modes

| Mode | What it contains | When to use |
|---|---|---|
| **Snapshot** | Full state + full history | Persistence to disk, initial load for new peers |
| **Updates** | Ops since a VersionVector | Real-time delta sync between peers |
| **ShallowSnapshot** | State + history since Frontiers | Storage optimization, privacy (trim old history) |
| **StateOnly** | Current state, minimal/no history | Transfer state without history |
| **SnapshotAt** | Full history up to Frontiers | Historical snapshot at a specific version |
| **UpdatesInRange** | Ops in specific ID spans | Selective history export |

```rust
// Rust
use loro::ExportMode;

doc.export(ExportMode::Snapshot).unwrap();
doc.export(ExportMode::all_updates()).unwrap();
doc.export(ExportMode::updates(&their_vv)).unwrap();
doc.export(ExportMode::shallow_snapshot(&frontiers)).unwrap();
doc.export(ExportMode::state_only(Some(&frontiers))).unwrap();
doc.export(ExportMode::snapshot_at(&frontiers)).unwrap();
```

```typescript
// TypeScript
doc.export({ mode: "snapshot" });
doc.export({ mode: "update" });
doc.export({ mode: "update", from: theirVV });
doc.export({ mode: "shallow-snapshot", frontiers });
doc.export({ mode: "state-only" });
```

Docs: [ExportMode (Rust)](https://docs.rs/loro/latest/loro/enum.ExportMode.html) | [Encoding tutorial](https://loro.dev/docs/tutorial/encoding)

## Persistence Pattern

A common persistence strategy: periodic snapshots + appended updates between snapshots.

1. On startup: load the latest snapshot, then import all updates recorded after it.
2. On every local change: append the update bytes (from `subscribe_local_update`) to storage.
3. Periodically: re-export a full snapshot and clear the update log.

This balances fast incremental saves with compact long-term storage.

## Shallow Snapshots

Shallow snapshots trim history before a given Frontiers boundary. Like `git clone --depth=1`.

```rust
// Rust
let shallow = doc.export(ExportMode::shallow_snapshot(&doc.oplog_frontiers())).unwrap();
let shallow_doc = LoroDoc::from_snapshot(&shallow).unwrap();
assert!(shallow_doc.is_shallow());
```

**Constraints**: A shallow doc cannot import updates that are concurrent to or before its shallow start version. It can only import updates that causally follow the shallow boundary. Plan your sync boundaries accordingly.

Docs: [Shallow Snapshots](https://loro.dev/docs/advanced/shallow_snapshot)

## Diff and Apply

Compute a diff between two versions and apply it to another doc. Useful for squash-like workflows:

```rust
// Rust
let diff = new_doc.diff(&base_frontiers, &new_doc.state_frontiers()).unwrap();
base_doc.apply_diff(diff).unwrap();
```

## Data Format Stability

Loro's binary encoding format is stabilized as of v1.0. No planned breaking changes to the wire format. All exported bytes include a checksum header; corrupted data is rejected during import.
