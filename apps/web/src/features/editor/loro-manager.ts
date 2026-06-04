// LoroClientManager: one campaign-scoped WebSocket, many CRDT rooms multiplexed
// over it. This is the "Repo" pattern (from loro-extended / SchoolAI): room
// join/leave lives here, not in React effects, so the doc + socket lifecycle is
// decoupled from component mount cycles. React hooks are read-only subscriptions
// (useThingDoc / useToc via useSyncExternalStore).
//
// Why one socket: no WebSocket handshake + auth + full-snapshot re-pull on every
// navigation, and it is shared by every room (the ToC and each open Thing).
//
// ── One room abstraction ─────────────────────────────────────────────────────
//
// Every room is the same thing: a doc, a join lifecycle, a snapshot, and a set
// of subscribers. Rooms differ only in two orthogonal policies:
//   - lifetime: ref-counted with a debounced leave. A "pinned" room (the ToC) is
//     just a room the layout acquires once and holds for the campaign's life.
//   - view: what subscribers read, derived from the doc by a `select(doc) => T`.
//     A Thing room's view is the LoroDoc itself (the editor syncs it through
//     loro-prosemirror); the ToC room's view is the derived tree, recomputed on
//     every commit/import. So Thing and ToC are two call sites with different
//     config, not two code paths.
//
// ── Lifecycle: one client, never swapped ─────────────────────────────────────
//
// The underlying loro-websocket client is constructed ONCE (lazily, on the first
// connect() or room acquire) and reused for the manager's whole life. It already
// owns the hard parts: auto-reconnect with backoff, browser offline/online, and
// re-joining active rooms (preserving their docs) when the socket returns. We do
// not reimplement any of that; we surface its per-room status changes into the
// snapshot so the UI can show "reconnecting" instead of silently freezing.
//
// The one thing we must not do is tear that client down on React StrictMode's
// mount -> cleanup -> remount. So close() schedules a debounced teardown that a
// remount's connect() cancels; the single client survives and the rooms map is
// never cleared mid-cycle. The same debounce on per-room leaves means a quick
// release -> re-acquire never tears a room down either.
//
// Because the client is never swapped and each room keeps exactly one doc joined
// once, room joins are naturally idempotent. loro-websocket's join() also dedups
// by (crdtType + roomId), but with one-doc-per-room that dedup is a harmless
// no-op rather than something to defend against.
//
// The two vendored patches (loro-prosemirror, loro-websocket) sit below this at
// the binding/transport layer and are unaffected by where join/leave is driven.

import { TOC_CONTAINER } from "@familiar-systems/types-campaign";
import type { ThingId } from "@familiar-systems/types-campaign";
import { LoroAdaptor } from "loro-adaptors/loro";
import type { LoroDoc as LoroDocType, TreeID } from "loro-crdt";
import { LoroDoc } from "loro-crdt";
import { LoroWebsocketClient } from "loro-websocket";
import type { LoroWebsocketClientRoom, RoomJoinStatusValue } from "loro-websocket";

import { moveTocNode as applyTocMove, readTocTree } from "../toc/toc-doc";
import type { TocTreeNode } from "../toc/toc-doc";

// Public, referentially-stable snapshot read by useSyncExternalStore. `view` is
// the room's projection of its doc (a LoroDoc for Thing rooms, a TocTreeNode[]
// for the ToC). `reconnecting` carries the last-known view while the socket
// recovers, so consumers keep rendering rather than dropping to a loading state.
export type RoomSnapshot<T> =
  | { status: "joining" }
  | { status: "joined"; view: T }
  | { status: "reconnecting"; view: T }
  | { status: "error"; message: string };

// The room's transport lifecycle, from acquire to a live (or failed) join. A
// room starts `joining` (a doc exists, but no socket-level room yet), reaches
// `bound` once the server-version handoff completes, or ends `failed` if the
// join errors. `bound` is the only state that owns a `room` and (for derived
// rooms) the doc subscription, so the room object and its subscription always
// travel together: there is no representable "joined but roomless" state to keep
// consistent by hand, and consumers of the binding get the room's lifetime
// guarantees from the variant rather than from a flag. `bound` spans both the
// "joined" and "reconnecting" snapshots - a socket drop keeps the room object
// alive while the doc is preserved, so the lifecycle does not regress to
// `joining`. Same shape as the server's `Persist`/`Occupancy` enums.
type Binding =
  | { kind: "joining" }
  | { kind: "bound"; room: LoroWebsocketClientRoom; docUnsub: (() => void) | null }
  | { kind: "failed" };

// Internal bookkeeping for one room. A ref-counted registry is inherently
// mutable; the public-facing discriminated union lives in `snapshot`, the
// transport-facing one in `binding`. They stay separate: `snapshot` must be a
// stable reference for useSyncExternalStore and carries the `reconnecting`
// sub-state and the view `T`, neither of which `binding` has. `refCount` and
// `leaveTimer` are an orthogonal lifetime concern (a counter and a debounce
// timer), not part of the join-state product, so they remain plain fields.
interface RoomHandle<T> {
  readonly roomId: string;
  // Created once and reused for the room's life, including across reconnects
  // (loro-websocket preserves it on rejoin).
  readonly doc: LoroDocType;
  readonly select: (doc: LoroDocType) => T;
  // Whether to subscribe to the doc and recompute the view on every change. The
  // ToC's derived tree needs it; a Thing room's view is the stable doc itself.
  readonly derived: boolean;
  refCount: number;
  binding: Binding;
  snapshot: RoomSnapshot<T>;
  leaveTimer: ReturnType<typeof setTimeout> | null;
}

// Debounce window for both per-room leaves and the socket teardown. Long enough
// to absorb StrictMode's synchronous unmount -> remount and rapid back-and-forth
// navigation; short enough to be invisible at human pace.
const LEAVE_DEBOUNCE_MS = 100;

// Single frozen instance so a not-yet-acquired room returns a stable reference
// (otherwise useSyncExternalStore would loop forever). Assignable to any
// RoomSnapshot<T> because the "joining" variant carries no view.
const JOINING: RoomSnapshot<never> = Object.freeze({ status: "joining" });

function errMessage(err: unknown): string {
  return err instanceof Error ? err.message : "Failed to connect.";
}

export class LoroClientManager {
  // Constructed once (lazily) and reused for the manager's life; only the
  // debounced teardown nulls it. See the header note on why it is never swapped.
  private client: LoroWebsocketClient | null = null;
  // Pending debounced teardown; a connect() cancels it (StrictMode remount).
  private closeTimer: ReturnType<typeof setTimeout> | null = null;

  // The room registry. Heterogeneous (Thing rooms view a LoroDoc, the ToC a
  // TocTreeNode[]); see `room()` for how a view type is recovered on read.
  private readonly rooms = new Map<string, RoomHandle<unknown>>();
  // Subscribers per room id. Kept separate from `rooms` so useSyncExternalStore
  // can subscribe before (or after) the acquire effect creates the handle.
  private readonly listeners = new Map<string, Set<() => void>>();

  constructor(private readonly wsUrl: string) {
    // Pure: no socket. The first connect() or acquire constructs the client.
  }

  // ---- connection lifecycle (provider-owned) ------------------------------

  /** Construct the client if needed; idempotent. The socket auto-connects. */
  private ensureClient(): LoroWebsocketClient {
    if (!this.client) this.client = new LoroWebsocketClient({ url: this.wsUrl });
    return this.client;
  }

  /**
   * Open the socket. Idempotent: a second call (e.g. StrictMode remount) cancels
   * a pending teardown and is otherwise a no-op. Rooms are joined by their
   * consumers' acquires, not here.
   */
  connect(): void {
    if (this.closeTimer != null) {
      clearTimeout(this.closeTimer);
      this.closeTimer = null;
    }
    this.ensureClient();
  }

  /**
   * Schedule a debounced teardown of the socket and all rooms. A connect()
   * inside the window cancels it, so StrictMode's connect -> close -> connect
   * tears nothing down. Idempotent.
   */
  close(): void {
    if (!this.client || this.closeTimer != null) return;
    this.closeTimer = setTimeout(() => this.teardown(), LEAVE_DEBOUNCE_MS);
  }

  private teardown(): void {
    this.closeTimer = null;
    if (!this.client) return;
    for (const handle of this.rooms.values()) {
      if (handle.leaveTimer != null) clearTimeout(handle.leaveTimer);
      if (handle.binding.kind === "bound") {
        handle.binding.docUnsub?.();
        void handle.binding.room.destroy().catch(() => {});
      }
    }
    this.rooms.clear();
    this.client.destroy();
    this.client = null;
    // Wake any hooks so they re-read JOINING. Harmless if truly unmounting.
    for (const roomId of this.listeners.keys()) this.notify(roomId);
  }

  // ---- generic room core --------------------------------------------------

  // Recover a room's view type. The registry is heterogeneous, but a room's view
  // type T is fixed by the select() it was acquired with, so reading it back by
  // the caller's expected T is sound. The cast is confined to this one accessor.
  private room<T>(roomId: string): RoomHandle<T> | undefined {
    return this.rooms.get(roomId) as RoomHandle<T> | undefined;
  }

  /** Ref-counted acquire. Idempotent; retries after a failed join. */
  private acquire<T>(roomId: string, select: (doc: LoroDocType) => T, derived: boolean): void {
    const existing = this.rooms.get(roomId);
    if (existing && existing.snapshot.status !== "error") {
      existing.refCount++;
      if (existing.leaveTimer != null) {
        clearTimeout(existing.leaveTimer);
        existing.leaveTimer = null;
      }
      return;
    }
    // New room, or a re-acquire after a failed join: (re)start from scratch.
    const refCount = (existing?.refCount ?? 0) + 1;
    const handle: RoomHandle<T> = {
      roomId,
      doc: new LoroDoc(),
      select,
      derived,
      refCount,
      binding: { kind: "joining" },
      snapshot: JOINING,
      leaveTimer: null,
    };
    this.rooms.set(roomId, handle);
    this.notify(roomId);
    void this.joinRoom<T>(roomId);
  }

  /** Ref-counted release. Debounced leave on the last release. */
  private release(roomId: string): void {
    const handle = this.rooms.get(roomId);
    if (!handle) return;
    handle.refCount--;
    if (handle.refCount > 0) return;

    switch (handle.snapshot.status) {
      case "joined":
      case "reconnecting":
        handle.leaveTimer = setTimeout(() => this.leaveRoom(roomId), LEAVE_DEBOUNCE_MS);
        break;
      case "error":
        // Nothing joined to tear down; drop it so a future acquire is fresh.
        this.rooms.delete(roomId);
        break;
      case "joining":
        // The in-flight joinRoom sees refCount <= 0 on completion and cleans up.
        break;
    }
  }

  private async joinRoom<T>(roomId: string): Promise<void> {
    // ensureClient() is synchronous, so a deep link that acquires a room before
    // the provider's connect() opens the socket here rather than failing.
    const client = this.ensureClient();

    const wanted = this.room<T>(roomId);
    if (!wanted || wanted.refCount <= 0) {
      this.rooms.delete(roomId);
      return;
    }

    try {
      await client.waitConnected();
      const room = await client.join({
        roomId,
        crdtAdaptor: new LoroAdaptor(wanted.doc),
        onStatusChange: (s) => this.onRoomStatus(roomId, s),
      });
      // Resolves once the server snapshot is applied locally, so consumers mount
      // against fully-synced content (no empty-doc flash).
      await room.waitForReachingServerVersion();

      // The join is asynchronous, so by the time it completes the handle it was
      // started for may have been superseded: a debounced teardown can clear the
      // map and a re-acquire can install a fresh handle (with its own doc) under
      // the same roomId. The room must be committed only to the exact handle that
      // requested it. Identity is the key, not refCount, because a replacement
      // handle also carries refCount > 0; binding to anything but `wanted` would
      // attach this room (wired to `wanted.doc`) to a handle whose own doc would
      // then never receive updates.
      const current = this.room<T>(roomId);
      if (current !== wanted || wanted.refCount <= 0) {
        void room.destroy().catch(() => {});
        // Only reap the slot if WE are the released occupant; never touch a newer
        // handle that has taken our place.
        if (current === wanted && wanted.refCount <= 0) this.rooms.delete(roomId);
        return;
      }

      const docUnsub = wanted.derived ? wanted.doc.subscribe(() => this.recompute(roomId)) : null;
      wanted.binding = { kind: "bound", room, docUnsub };
      wanted.snapshot = { status: "joined", view: wanted.select(wanted.doc) };
      this.notify(roomId);
    } catch (err) {
      // Same identity rule on the failure path: a join that rejects after its
      // handle was superseded reports the error only if that handle is still the
      // live one, never onto the handle that replaced it.
      if (this.room<T>(roomId) === wanted) {
        wanted.binding = { kind: "failed" };
        wanted.snapshot = { status: "error", message: errMessage(err) };
        this.notify(roomId);
      }
    }
  }

  /** Re-derive a derived room's view after a local commit or remote import. */
  private recompute(roomId: string): void {
    const handle = this.rooms.get(roomId);
    if (!handle) return;
    const { status } = handle.snapshot;
    if (status !== "joined" && status !== "reconnecting") return;
    handle.snapshot = { status, view: handle.select(handle.doc) };
    this.notify(roomId);
  }

  /**
   * Fold loro-websocket's per-room status into the snapshot. Only once the room
   * is `bound`: the library emits Connecting/Joined during our own join(), which
   * the explicit waitForReachingServerVersion handoff already owns, so folding
   * before the handoff would let that first status pre-empt it. Reconnecting and
   * Disconnected both surface as "reconnecting" (socket down, doc preserved).
   */
  private onRoomStatus(roomId: string, status: RoomJoinStatusValue): void {
    const handle = this.rooms.get(roomId);
    if (!handle || handle.binding.kind !== "bound") return;
    switch (status) {
      case "reconnecting":
      case "disconnected":
        if (handle.snapshot.status !== "reconnecting") {
          handle.snapshot = { status: "reconnecting", view: handle.select(handle.doc) };
          this.notify(roomId);
        }
        break;
      case "joined":
        if (handle.snapshot.status !== "joined") {
          handle.snapshot = { status: "joined", view: handle.select(handle.doc) };
          this.notify(roomId);
        }
        break;
      case "error":
        handle.snapshot = { status: "error", message: "Connection lost." };
        this.notify(roomId);
        break;
      case "connecting":
        break;
    }
  }

  private leaveRoom(roomId: string): void {
    const handle = this.rooms.get(roomId);
    if (!handle) return;
    // `bound` is exactly the joined-or-reconnecting state this debounced leave was
    // scheduled for; a still-joining or failed room has no room object to drop.
    if (handle.binding.kind !== "bound") return;
    this.rooms.delete(roomId);
    handle.binding.docUnsub?.();
    // room.destroy() leaves the room and removes it from the client's dedup table
    // within a microtask, before any later navigation's re-acquire.
    void handle.binding.room.destroy().catch(() => {});
    this.notify(roomId);
  }

  // ---- useSyncExternalStore plumbing --------------------------------------

  private subscribeRoom(roomId: string, listener: () => void): () => void {
    let set = this.listeners.get(roomId);
    if (!set) {
      set = new Set();
      this.listeners.set(roomId, set);
    }
    set.add(listener);
    return () => {
      set.delete(listener);
      if (set.size === 0) this.listeners.delete(roomId);
    };
  }

  private notify(roomId: string): void {
    this.listeners.get(roomId)?.forEach((l) => l());
  }

  // ---- Thing rooms (useThingDoc-owned) ------------------------------------

  /** Called from useThingDoc's mount effect. The view is the doc itself. */
  acquireThing(id: ThingId): void {
    this.acquire(`thing:${id}`, (doc) => doc, false);
  }

  /** Called from useThingDoc's cleanup. */
  releaseThing(id: ThingId): void {
    this.release(`thing:${id}`);
  }

  subscribeThingDoc = (id: ThingId, listener: () => void): (() => void) =>
    this.subscribeRoom(`thing:${id}`, listener);

  getThingState = (id: ThingId): RoomSnapshot<LoroDocType> =>
    this.room<LoroDocType>(`thing:${id}`)?.snapshot ?? JOINING;

  // ---- ToC room (layout-pinned) -------------------------------------------

  /** Pin the ToC room. The campaign layout holds the single long-lived acquire. */
  acquireToc(): void {
    this.acquire(TOC_CONTAINER, readTocTree, true);
  }

  releaseToc(): void {
    this.release(TOC_CONTAINER);
  }

  subscribeToc = (listener: () => void): (() => void) =>
    this.subscribeRoom(TOC_CONTAINER, listener);

  getTocSnapshot = (): RoomSnapshot<TocTreeNode[]> =>
    this.room<TocTreeNode[]>(TOC_CONTAINER)?.snapshot ?? JOINING;

  /**
   * Move a ToC node under `parent` (root when null) at sibling `index`. The local
   * commit syncs over the room and optimistically updates the snapshot through the
   * doc subscription. No-op until the ToC room has joined.
   */
  moveTocNode = (node: TreeID, parent: TreeID | null, index: number): void => {
    const handle = this.room<TocTreeNode[]>(TOC_CONTAINER);
    if (handle?.binding.kind !== "bound") return;
    applyTocMove(handle.doc, node, parent, index);
  };
}
