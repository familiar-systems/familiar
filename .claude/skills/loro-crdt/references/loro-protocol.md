# Loro Protocol

The `loro-protocol` library provides a WebSocket sync protocol for Loro documents: wire format, handshake, room management, fragmentation, and an adaptor trait for plugging in your own LoroDoc management.

**The crates.io version is stale.** Use a Git dependency for Rust. See [github.com/loro-dev/protocol/issues/57](https://github.com/loro-dev/protocol/issues/57).

```toml
# Rust: use git dependency
[dependencies]
loro-protocol = { git = "https://github.com/loro-dev/protocol", path = "rust/loro-protocol" }
loro-websocket-client = { git = "https://github.com/loro-dev/protocol", path = "rust/loro-websocket-client" }
```

```bash
# TypeScript
npm install loro-protocol loro-websocket loro-adaptors
```

## Authoritative References

- [Blog post (design rationale)](https://loro.dev/blog/loro-protocol)
- [GitHub repo](https://github.com/loro-dev/protocol)
- [Wire spec (protocol.md)](https://github.com/loro-dev/protocol/blob/main/protocol.md)
- [LLM reference (llms.md)](https://github.com/loro-dev/protocol/blob/main/llms.md)
- [E2EE extension spec](https://github.com/loro-dev/protocol/blob/main/protocol-e2ee.md)

## Wire Format Overview

Every binary message has this envelope:

```
[4 bytes: CRDT magic] [varString: room_id] [1 byte: message_type] [payload...]
```

**CRDT magic bytes**: `%LOR` (Loro doc), `%EPH` (ephemeral store), `%EPS` (persisted ephemeral), `%ELO` (E2E encrypted Loro).

**Room identity** = `(CrdtType, room_id)`. Same room_id with different CRDT types are distinct rooms.

**Max message size**: 256 KiB. Larger payloads are automatically fragmented.

**Keepalive**: Text frames containing exactly `"ping"` or `"pong"` bypass the envelope entirely. Connection-scoped, never broadcast.

Full spec: [protocol.md](https://github.com/loro-dev/protocol/blob/main/protocol.md)

## Message Types

Source: `rust/loro-protocol/src/protocol.rs`

```rust
pub enum CrdtType {
    Loro,                        // %LOR
    LoroEphemeralStore,          // %EPH
    LoroEphemeralStorePersisted, // %EPS
    Elo,                         // %ELO
}

pub struct BatchId(pub [u8; 8]);

pub enum ProtocolMessage {
    JoinRequest {
        crdt: CrdtType,
        room_id: String,
        auth: Vec<u8>,       // application-defined (e.g. JWT, session token)
        version: Vec<u8>,    // your LoroDoc's current version
    },
    JoinResponseOk {
        crdt: CrdtType,
        room_id: String,
        permission: Permission, // Read | Write
        version: Vec<u8>,      // server's current version
        extra: Option<Vec<u8>>,
    },
    JoinError {
        crdt: CrdtType,
        room_id: String,
        code: JoinErrorCode,
        message: String,
        receiver_version: Option<Vec<u8>>, // present when code == VersionUnknown
        app_code: Option<String>,          // present when code == AppError
    },
    DocUpdate {
        crdt: CrdtType,
        room_id: String,
        updates: Vec<Vec<u8>>,  // one or more update chunks
        batch_id: BatchId,      // for Ack correlation
    },
    DocUpdateFragmentHeader {
        crdt: CrdtType,
        room_id: String,
        batch_id: BatchId,
        fragment_count: u64,
        total_size_bytes: u64,
    },
    DocUpdateFragment {
        crdt: CrdtType,
        room_id: String,
        batch_id: BatchId,
        index: u64,
        fragment: Vec<u8>,
    },
    RoomError {
        crdt: CrdtType,
        room_id: String,
        code: RoomErrorCode,
        message: String,
    },
    Ack {
        crdt: CrdtType,
        room_id: String,
        ref_id: BatchId,         // references the DocUpdate's batch_id
        status: UpdateStatusCode,
    },
    Leave {
        crdt: CrdtType,
        room_id: String,
    },
}
```

## Encode / Decode

The `loro-protocol` crate's entire public API:

```rust
use loro_protocol::{encode, decode, try_decode, ProtocolMessage};

let msg = ProtocolMessage::Leave { crdt: CrdtType::Loro, room_id: "doc:1".into() };
let bytes: Vec<u8> = encode(&msg).unwrap();
let decoded: ProtocolMessage = decode(&bytes).unwrap();
let maybe: Option<ProtocolMessage> = try_decode(&bytes);
```

Source: `rust/loro-protocol/src/encoding.rs`

## CrdtDocAdaptor Trait

The main extension point for the Rust WebSocket client. Implement this to control how your LoroDoc interacts with the protocol.

Source: `rust/loro-websocket-client/src/adaptor.rs`

```rust
#[async_trait]
pub trait CrdtDocAdaptor {
    // Required:
    fn crdt_type(&self) -> CrdtType;
    async fn version(&self) -> Vec<u8>;
    async fn set_ctx(&mut self, ctx: CrdtAdaptorContext);
    async fn handle_join_ok(&mut self, permission: Permission, version: Vec<u8>);
    async fn apply_update(&mut self, updates: Vec<Vec<u8>>);

    // Optional hooks:
    async fn handle_ack(&mut self, ref_id: BatchId, status: UpdateStatusCode) {}
    async fn handle_update_error(&mut self, updates: Vec<Vec<u8>>,
        status: UpdateStatusCode, reason: Option<String>) {}
    async fn handle_room_error(&mut self, code: RoomErrorCode, message: &str) {}
    async fn handle_join_err(&mut self, code: JoinErrorCode, message: &str) {}
    async fn get_alternative_version(&mut self, current: &[u8]) -> Option<Vec<u8>> { None }
}

pub struct CrdtAdaptorContext {
    pub send_update: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    pub on_join_failed: Arc<dyn Fn(String) + Send + Sync>,
    pub on_import_error: Arc<dyn Fn(String, Vec<Vec<u8>>) + Send + Sync>,
}
```

The `send_update` closure handles automatic fragmentation when payload exceeds 256 KiB.

## Built-in Adaptors

**`LoroDocAdaptor`** (`%LOR`): Wraps `Arc<Mutex<LoroDoc>>`. On `set_ctx`, subscribes to `doc.subscribe_local_update()` to auto-send edits. On `apply_update`, calls `doc.import()`. On `handle_join_ok`, exports updates from the server's version and sends them, then imports the server's backfill.

**`EloDocAdaptor`** (`%ELO`): Same as above but encrypts outgoing updates with AES-256-GCM and decrypts incoming ones. Requires a `key_id: String` and `key: [u8; 32]`.

## LoroWebsocketClient (Rust)

Source: `rust/loro-websocket-client/src/client.rs`

```rust
// Connect
let client = LoroWebsocketClient::connect("ws://localhost:8787/workspace").await?;
let client = LoroWebsocketClient::connect_with_config(url, ClientConfig {
    fragment_reassembly_timeout: Duration::from_secs(10),
    ..Default::default()
}).await?;

// Join a %LOR room (auto-syncs via subscribe_local_update)
let doc = Arc::new(Mutex::new(LoroDoc::new()));
let room = client.join_loro("room-id", doc.clone()).await?;

// Join with E2EE
let room = client.join_elo_with_adaptor(
    "room-id", doc.clone(), "key-v1", aes_key
).await?;

// Join with custom adaptor
let room = client.join_with_adaptor("room-id", Box::new(my_adaptor)).await?;

// Leave
room.leave().await?;
```

After `join_loro`, edits to the LoroDoc followed by `doc.commit()` are automatically sent over the WebSocket. Incoming updates from other peers are automatically imported into the doc.

## LoroWebsocketClient (TypeScript)

```typescript
import { LoroWebsocketClient } from "loro-websocket";
import { LoroAdaptor } from "loro-adaptors";
import { LoroDoc } from "loro-crdt";

const client = new LoroWebsocketClient({ url: "ws://localhost:8787/workspace" });
await client.waitConnected();

const doc = new LoroDoc();
const adaptor = new LoroAdaptor(doc);
const room = await client.join({ roomId: "room-id", crdtAdaptor: adaptor });

// Edits auto-sync after commit
doc.getText("t").insert(0, "hello");
doc.commit();

// Cleanup
await room.destroy();
client.close();    // graceful, disables auto-reconnect
client.destroy();  // full teardown
```

The TS client includes **auto-reconnect** with exponential backoff (500ms base, 15s cap). The Rust client does not; reconnection is your responsibility.

## Handshake Flow

```
Client                              Server
  |                                   |
  |-- JoinRequest(auth, version) ---->|
  |                                   |  (authenticate, load doc)
  |<---- JoinResponseOk(perm, ver) ---|
  |<---- DocUpdate(backfill) ---------|  (snapshot or delta to catch up)
  |                                   |
  |-- DocUpdate(local edits) -------->|
  |<---- Ack(batch_id, ok) ----------|
  |                                   |
  |<---- DocUpdate(from peer B) -----|  (broadcast, no Ack expected back)
  |                                   |
  |-- Leave ------------------------->|
```

**Ack rules**: The server sends Ack for every client-originated DocUpdate. Clients do NOT ack server broadcasts (unless they fail to apply, in which case send a non-zero status Ack).

On `JoinError(version_unknown)`, the client tries `adaptor.get_alternative_version()`, then falls back to an empty version vector.

On `RoomError(rejoin_suggested)`, the client may auto-rejoin once. On `RoomError(evicted)`, the client must not auto-rejoin.

## Error Codes

**JoinError** (`JoinErrorCode`):

| Code | Name | Notes |
|------|------|-------|
| `0x00` | Unknown | |
| `0x01` | VersionUnknown | Response includes `receiver_version` for reseeding |
| `0x02` | AuthFailed | |
| `0x7F` | AppError | Includes `app_code` string |

**RoomError** (`RoomErrorCode`):

| Code | Name | Client behavior |
|------|------|-----------------|
| `0x01` | RejoinSuggested | MAY auto-rejoin once |
| `0x02` | Evicted | MUST NOT auto-rejoin |
| `0x7F` | Unknown | Treat as fatal |

**UpdateStatus** (`UpdateStatusCode`):

| Code | Name |
|------|------|
| `0x00` | Ok |
| `0x01` | Unknown |
| `0x03` | PermissionDenied |
| `0x04` | InvalidUpdate |
| `0x05` | PayloadTooLarge |
| `0x06` | RateLimited |
| `0x07` | FragmentTimeout |
| `0x7F` | AppError |

## Fragmentation

The client automatically fragments payloads over 256 KiB:

1. Sends `DocUpdateFragmentHeader` with batch_id, fragment count, total size
2. Sends N `DocUpdateFragment` messages with the same batch_id
3. Receiver reassembles by batch_id; 10-second default timeout
4. On timeout: receiver sends `Ack(fragment_timeout)`; sender should resend

`ClientConfig` controls fragment behavior:
```rust
pub struct ClientConfig {
    pub fragment_reassembly_timeout: Duration,  // default 10s
    pub fragment_limit_headroom: usize,         // default 4096 bytes
    pub fragment_limit_soft_max: usize,         // default 240 KiB per fragment
}
```

## Ephemeral State

Two CRDT types for ephemeral/presence data, both using `loro::awareness::EphemeralStore` internally:

- **`%EPH`**: Server relays but does not persist. Cleaned up when last subscriber leaves.
- **`%EPS`**: Server persists latest state via hooks. Late joiners get hydrated even if no other peer is connected.

```typescript
// TypeScript
import { LoroEphemeralAdaptor } from "loro-adaptors";
const adaptor = new LoroEphemeralAdaptor();
const room = await client.join({ roomId: "doc:123", crdtAdaptor: adaptor });
adaptor.getStore().set("cursor", { line: 5, col: 12 });
```

## What the Protocol Does NOT Provide

- **No auto-reconnect in the Rust client.** The TS client has exponential backoff; the Rust client is connect-once. Build your own reconnection logic.
- **No rate limiting.** The `RateLimited` status code exists, but no built-in enforcement. Application concern.
- **No room discovery.** The protocol syncs individual rooms. "Which rooms exist?" is outside its scope.
- **No key management for E2EE.** Key distribution, rotation, and agreement are application-level. The protocol only encrypts/decrypts with a provided key.
- **No conflict resolution beyond CRDT semantics.** The protocol transports opaque bytes; the `loro` crate handles merging.
