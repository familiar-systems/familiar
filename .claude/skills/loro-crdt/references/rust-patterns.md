# Loro Rust Patterns

## Setup

```toml
[dependencies]
loro = "1"
# Optional features:
# loro = { version = "1", features = ["counter", "jsonpath"] }
```

Full API: [docs.rs/loro](https://docs.rs/loro/latest/loro/)

## Container Access

Root containers are created implicitly. Accessing one does not produce history.

```rust
use loro::LoroDoc;

let doc = LoroDoc::new();
let map = doc.get_map("config");       // LoroMap
let list = doc.get_list("items");      // LoroList
let text = doc.get_text("content");    // LoroText
let tree = doc.get_tree("hierarchy");  // LoroTree
let ml = doc.get_movable_list("tasks");// LoroMovableList
let ctr = doc.get_counter("visits");   // LoroCounter (feature "counter")

// Safe variants that return None if container doesn't exist:
let maybe = doc.try_get_map("config"); // Option<LoroMap>
```

All container types implement `Send + Sync`.

Docs: [LoroDoc](https://docs.rs/loro/latest/loro/struct.LoroDoc.html)

## ContainerTrait

Every container type implements this trait:

```rust
container.id()            // -> ContainerID
container.is_attached()   // attached to a LoroDoc?
container.get_attached()  // -> Option<Self>, the attached version
container.is_deleted()    // has this container been deleted?
container.doc()           // -> Option<LoroDoc>
```

## Detached Containers

`LoroText::new()`, `LoroList::new()`, etc. create **detached** containers. Edits on them are buffered locally and not recorded in any document's history until inserted into an attached container.

```rust
let text = LoroText::new();
text.insert(0, "draft").unwrap(); // buffered, no history

let doc = LoroDoc::new();
let map = doc.get_map("root");
let attached_text = map.insert_container("content", text).unwrap();
// Now `attached_text` is attached; the buffered edit is applied
```

Use `get_or_create_container` on LoroMap for idempotent sub-container access:
```rust
let list = map.get_or_create_container("items", LoroList::new()).unwrap();
```

## LoroValue

The universal value enum stored in containers:

```rust
pub enum LoroValue {
    Null,
    Bool(bool),
    Double(f64),
    I64(i64),
    String(Arc<str>),
    Binary(Arc<Vec<u8>>),
    List(Arc<Vec<LoroValue>>),
    Map(Arc<FxHashMap<String, LoroValue>>),
    Container(ContainerID),
}
```

The `loro_value!` macro works like `serde_json::json!`:
```rust
use loro::loro_value;
let v = loro_value!({ "name": "Alice", "age": 30, "scores": [95, 87] });
```

Docs: [LoroValue](https://docs.rs/loro/latest/loro/enum.LoroValue.html)

## ValueOrContainer

Returned by container `get()` methods. Distinguish between a stored primitive and a nested container:

```rust
use loro::ValueOrContainer;
match map.get("key") {
    Some(ValueOrContainer::Value(v)) => { /* LoroValue */ }
    Some(ValueOrContainer::Container(c)) => { /* Container enum */ }
    None => { /* key doesn't exist */ }
}
```

## Commits

Edits are buffered until `commit()`. Events fire after commit.

```rust
doc.get_text("t").insert(0, "hello").unwrap();
doc.commit(); // finalizes pending ops, fires subscriptions

// With metadata:
doc.set_next_commit_message("add greeting");
doc.set_next_commit_origin("user-action"); // local-only, not persisted
doc.set_next_commit_timestamp(1700000000);
doc.commit();

// Or use CommitOptions:
use loro::CommitOptions;
doc.commit_with(CommitOptions {
    origin: Some("batch-import".into()),
    ..Default::default()
});
```

## Subscriptions

Subscriptions auto-unsubscribe when the `Subscription` object is dropped. Hold onto it for the lifetime you need.

```rust
use std::sync::Arc;

// All changes on the doc
let _sub = doc.subscribe_root(Arc::new(|event| {
    for e in &event.events {
        println!("container {:?} changed", e.container);
    }
}));

// Specific container
let _sub = doc.subscribe(&text.id(), Arc::new(move |event| {
    for e in &event.events {
        if let Some(delta) = e.diff.as_text() {
            // process TextDelta
        }
    }
}));

// Real-time sync: fires with raw bytes on every local change
let _sub = doc.subscribe_local_update(Box::new(move |bytes| {
    send_to_peer(bytes);
    true // return false to auto-unsubscribe
}));

// Pre-commit hook
let _sub = doc.subscribe_pre_commit(Box::new(move |payload| {
    payload.modifier.set_message("auto-tagged");
    true
}));
```

Docs: [Events tutorial](https://loro.dev/docs/tutorial/event)

## UndoManager

Per-peer undo/redo. Only undoes operations from the current peer.

```rust
use loro::UndoManager;

let undo = UndoManager::new(&doc);
undo.set_max_undo_steps(100);
undo.set_merge_interval(1000); // ms

doc.get_text("t").insert(0, "Hello").unwrap();
doc.commit();

undo.undo().unwrap();
undo.redo().unwrap();

// Grouping: multiple commits undo as one step
undo.group_start();
doc.get_text("t").insert(0, "A").unwrap();
doc.commit();
doc.get_text("t").insert(1, "B").unwrap();
doc.commit();
undo.group_end();
```

The UndoManager tracks a single peer. If `set_peer_id` changes the doc's peer, both undo/redo stacks are cleared.

Use `add_exclude_origin_prefix("sys:")` to prevent system-originated changes from being undoable.

Docs: [UndoManager](https://docs.rs/loro/latest/loro/struct.UndoManager.html) | [Undo tutorial](https://loro.dev/docs/advanced/undo)

## ExportMode

```rust
use loro::ExportMode;

// Full state + full history (persistence, initial load)
doc.export(ExportMode::Snapshot).unwrap();

// Ops since a version vector (real-time delta sync)
doc.export(ExportMode::updates(&their_vv)).unwrap();

// All ops ever (equivalent to updates from empty VV)
doc.export(ExportMode::all_updates()).unwrap();

// Recent history only (storage optimization, privacy)
doc.export(ExportMode::shallow_snapshot(&frontiers)).unwrap();

// Current state without history
doc.export(ExportMode::state_only(Some(&frontiers))).unwrap();

// Full history up to a specific version
doc.export(ExportMode::snapshot_at(&frontiers)).unwrap();
```

Docs: [ExportMode](https://docs.rs/loro/latest/loro/enum.ExportMode.html) | [Encoding tutorial](https://loro.dev/docs/tutorial/encoding)

## Time Travel

```rust
let v0 = doc.state_frontiers();
// ... edits ...

// Read-only checkout (detached mode)
doc.checkout(&v0).unwrap();
doc.checkout_to_latest(); // or doc.attach()

// Revert: generates inverse ops, stays attached
doc.revert_to(&v0).unwrap();

// Deep copy at a version
let fork = doc.fork();            // at current state
let fork_at = doc.fork_at(&v0);   // at specific frontiers

// Allow editing in detached mode (uses different PeerID per checkout)
doc.set_detached_editing(true);
doc.checkout(&v0).unwrap();
doc.get_text("t").insert(0, "edit the past").unwrap();
doc.attach();
```

Docs: [Time Travel tutorial](https://loro.dev/docs/tutorial/time_travel)

## Rich Text Configuration

Must be called before inserting marks. Configure per mark key, once per doc.

```rust
use loro::{StyleConfigMap, StyleConfig, ExpandType};

let mut styles = StyleConfigMap::new();
styles.insert("bold".into(), StyleConfig { expand: ExpandType::After });
styles.insert("italic".into(), StyleConfig { expand: ExpandType::After });
styles.insert("link".into(), StyleConfig { expand: ExpandType::None });
styles.insert("comment".into(), StyleConfig { expand: ExpandType::Both });
doc.config_text_style(styles);

let text = doc.get_text("t");
text.insert(0, "Hello world").unwrap();
text.mark(0..5, "bold", true).unwrap();
text.mark(6..11, "link", "https://example.com").unwrap();
```

ExpandType controls what happens when text is inserted at a mark boundary:
- `After` (default): text at end inherits mark
- `Before`: text at start inherits mark
- `Both`: both boundaries expand
- `None`: no expansion

Docs: [Text tutorial](https://loro.dev/docs/tutorial/text)

## Path-Based Access

```rust
let val = doc.get_by_str_path("map/key");
let val = doc.get_by_path(&[Index::Key("map".into()), Index::Key("key".into())]);
```

## Memory Management

```rust
doc.compact_change_store();  // free parsed ops memory
doc.free_history_cache();    // free checkout acceleration cache
```
