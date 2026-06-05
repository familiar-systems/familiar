# Loro TypeScript Patterns

## Setup

```bash
npm install loro-crdt
# or
pnpm add loro-crdt
```

### Bundler Configuration

**Vite** (requires WASM plugins):
```bash
npm install vite-plugin-wasm vite-plugin-top-level-await
```
```typescript
// vite.config.ts
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
});
```

**Next.js**: requires `experiments.asyncWebAssembly = true` in `next.config.js`.

Full API: [JS API Reference](https://loro.dev/docs/api/js)

## Container Access

```typescript
import { LoroDoc } from "loro-crdt";

const doc = new LoroDoc();
const map = doc.getMap("config");       // LoroMap
const list = doc.getList("items");      // LoroList
const text = doc.getText("content");    // LoroText
const tree = doc.getTree("hierarchy");  // LoroTree
const ml = doc.getMovableList("tasks"); // LoroMovableList
const ctr = doc.getCounter("visits");   // LoroCounter
```

Root containers are created implicitly. Accessing one does not produce history.

## LoroValue Type

TypeScript's value type is a union, not an enum:

```typescript
type LoroValue =
  | null
  | boolean
  | number
  | string
  | Uint8Array
  | LoroValue[]
  | { [key: string]: LoroValue };
```

No `Arc` wrappers, no `I64` vs `Double` distinction. All numbers are JavaScript `number`.

## Setting Values vs Sub-Containers

A critical API distinction: primitives use `set`/`insert`, sub-containers use `setContainer`/`insertContainer`.

```typescript
// Primitives
map.set("name", "Alice");
map.set("age", 30);
list.insert(0, "item");
list.push("end");

// Sub-containers (returns the attached container)
const text = map.setContainer("bio", new LoroText());
const child = list.insertContainer(0, new LoroList());
const pushed = list.pushContainer(new LoroMap());

// Idempotent sub-container access
const items = map.getOrCreateContainer("items", new LoroList());
```

## Commits

```typescript
doc.getText("t").insert(0, "hello");
doc.commit(); // finalizes pending ops, fires subscriptions

// With metadata:
doc.commit({ origin: "user-action", message: "add greeting", timestamp: Date.now() });
```

## Subscriptions

Subscriptions return an **unsubscribe function** (not an object to drop).

```typescript
// All changes on the doc
const unsub = doc.subscribeRoot((event) => {
  for (const e of event.events) {
    console.log("container changed:", e.path);
  }
});

// Specific container
const unsub = doc.subscribe(text.id, (event) => {
  for (const e of event.events) {
    const delta = e.diff; // TextDelta[] for text containers
  }
});

// Real-time sync: fires with raw bytes on every local change
const unsub = doc.subscribeLocalUpdates((bytes: Uint8Array) => {
  websocket.send(bytes);
});

// Pre-commit hook (v1.5.0+)
const unsub = doc.subscribePreCommit((payload) => {
  payload.modifier.setMessage("auto-tagged");
});

// Cleanup
unsub(); // call the returned function to unsubscribe
```

Docs: [Events tutorial](https://loro.dev/docs/tutorial/event)

## EphemeralStore (v1.5.0+)

Replaces the older Awareness API. Timestamp-based LWW for ephemeral state (cursors, selections, presence). Not persisted in the document.

```typescript
import { EphemeralStore } from "loro-crdt";

const store = new EphemeralStore({ timeout: 30_000 }); // 30s TTL

// Set state for a peer
store.set(peerId, "cursor", { line: 5, col: 12 });
store.set(peerId, "name", "Alice");

// Get state
const cursor = store.get(peerId, "cursor");
const allPeers = store.getAllStates();

// Sync between peers
const bytes = store.encode(peerId);
remoteStore.apply(bytes);

// Subscribe to changes
const unsub = store.subscribe((event) => {
  // event.updated: Map<PeerId, Set<string>>
  // event.removed: Set<PeerId>
});

// Cleanup expired entries
store.removeOutdated();
```

Docs: [Awareness tutorial](https://loro.dev/docs/tutorial/awareness)

## UndoManager

```typescript
import { UndoManager } from "loro-crdt";

const undo = new UndoManager(doc, { mergeInterval: 1000, maxUndoSteps: 100 });
undo.addExcludeOriginPrefix("sys:");

doc.getText("t").insert(0, "Hello");
doc.commit();

undo.undo();
undo.redo();

// Grouping
undo.startGroup();
doc.getText("t").insert(0, "A");
doc.commit();
doc.getText("t").insert(1, "B");
doc.commit();
undo.stopGroup();

// Cursor restoration across undo/redo
undo.onPush = (isUndo, counterRange) => {
  return { cursor: getCurrentCursorState() }; // return metadata to persist
};
undo.onPop = (isUndo, meta, counterRange) => {
  restoreCursorState(meta.cursor); // restore on undo/redo
};
```

Docs: [Undo tutorial](https://loro.dev/docs/advanced/undo)

## Import / Export

```typescript
// Snapshot (full state + history)
const snapshot = doc.export({ mode: "snapshot" });
const restored = LoroDoc.fromSnapshot(snapshot);

// Updates since a version vector (delta sync)
const updates = doc.export({ mode: "update", from: theirVV });
doc.import(updates);

// All updates ever
const all = doc.export({ mode: "update" });

// Shallow snapshot (recent history only)
const shallow = doc.export({ mode: "shallow-snapshot", frontiers: doc.oplogFrontiers() });

// State only (no history)
const stateOnly = doc.export({ mode: "state-only" });

// JSON import/export
const json = doc.exportJsonUpdates(startVV, endVV);
doc.importJsonUpdates(json);
```

Docs: [Encoding tutorial](https://loro.dev/docs/tutorial/encoding)

## Time Travel

```typescript
const v0 = doc.frontiers();
// ... edits ...

doc.checkout(v0);         // read-only detached mode
doc.checkoutToLatest();   // or doc.attach()

// Fork at a version
const forked = doc.fork();
const forkedAt = doc.forkAt(v0);
```

Docs: [Time Travel tutorial](https://loro.dev/docs/tutorial/time_travel)

## Rich Text Configuration

Must be called before inserting marks.

```typescript
doc.configTextStyle({
  bold:    { expand: "after" },
  italic:  { expand: "after" },
  link:    { expand: "none" },
  comment: { expand: "both" },
});

const text = doc.getText("t");
text.insert(0, "Hello world");
text.mark({ start: 0, end: 5 }, "bold", true);
text.mark({ start: 6, end: 11 }, "link", "https://example.com");

// Delta output (Quill-compatible)
const delta = text.toDelta();
// [{ insert: "Hello", attributes: { bold: true } }, { insert: " " }, ...]
```

Docs: [Text tutorial](https://loro.dev/docs/tutorial/text)

## Tree Container

```typescript
const tree = doc.getTree("tree");
tree.enableFractionalIndex(0); // required for ordered siblings

const root = tree.createNode();          // root node (no parent)
const child = tree.createNode(root.id);  // child of root

// Move
tree.move(child.id, root.id);           // reparent
tree.moveAfter(child.id, sibling.id);   // reorder

// Metadata (each node has an associated LoroMap)
const meta = tree.getNodeByID(root.id);
meta.data.set("name", "Root");

// Query
tree.roots();     // root nodes
tree.children(root.id);
tree.toJSON();    // full hierarchy with metadata
```

Docs: [Tree tutorial](https://loro.dev/docs/tutorial/tree)

## PeerId

In TypeScript, PeerId is a `bigint` or decimal string, not a `u64`:

```typescript
doc.setPeerId(42n);      // bigint literal
doc.setPeerId("12345");  // decimal string
const id = doc.peerId;   // bigint
```

## Version Queries

```typescript
const oplogVV = doc.oplogVersion();       // VersionVector
const stateVV = doc.version();            // VersionVector (alias for stateVersion)
const frontiers = doc.frontiers();        // Frontiers
const oplogF = doc.oplogFrontiers();      // Frontiers
```
