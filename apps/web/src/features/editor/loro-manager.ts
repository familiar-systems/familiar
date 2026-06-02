// LoroClientManager: one campaign-scoped WebSocket, many CRDT rooms multiplexed
// over it. This is the "Repo" pattern (from loro-extended / SchoolAI): room
// join/leave lives here, not in React effects, so the doc + socket lifecycle is
// decoupled from component mount cycles. React hooks are read-only subscriptions
// (see LoroManagerProvider's useThingDoc + useSyncExternalStore).
//
// Why this over per-mount ownership: a single socket means no WebSocket
// handshake + auth + full-snapshot re-pull on every navigation, and it is shared
// by the always-on ToC room and the per-Thing rooms.
//
// Lifecycle ownership:
//   - connect()/close() are called by the provider's useEffect. The constructor
//     is pure (no socket) so React StrictMode's mount -> cleanup -> remount runs
//     connect -> close -> connect cleanly.
//   - acquireThing()/releaseThing() are called by useThingDoc's effect. Rooms are
//     ref-counted; the last release schedules a debounced leave (absorbs the
//     StrictMode unmount->remount and rapid back-and-forth without tearing the
//     room down). Debounce earns little for human-paced navigation but is kept
//     for a faithful port; it is dormant until there is more than one room.
//
// The two vendored patches (loro-prosemirror, loro-websocket) sit below this at
// the binding/transport layer and are unaffected by where join/leave is driven.
//
// ── Room joins must be idempotent per socket (a bug we must not reintroduce) ──
//
// loro-websocket's client.join() dedups by (crdtType + roomId): a second join
// for a room already pending/active on the same socket returns the FIRST join's
// room and silently discards the second call's adaptor + doc. Two joins for one
// room on one socket therefore do not produce two independent syncs; the second
// caller is left holding a doc that is never wired to the wire.
//
// StrictMode makes that reproducible rather than theoretical. The provider runs
// connect -> close -> connect; each connect() kicks off a room join, but both
// joins `await whenConnected()` first, and by the time they resume the
// synchronous connect/close/connect has already settled this.client to the
// second (surviving) socket. So both joins target that one socket and collide on
// the dedup above. The winner wires up and syncs; the loser awaits the winner's
// room version (which resolves), then reads its OWN still-empty doc and overwrites
// the good snapshot with nothing. Symptom: an empty view even though the server
// sent a full snapshot - so do not go looking for the bug on the server.
//
// The rule, for every room joined over this socket:
//   - Thing rooms: ref-counted via acquireThing, plus a per-entry `joinClient`
//     claim. Ref-count alone is not enough: on a hard refresh the provider's
//     close() clears the `things` map between StrictMode's two acquires, so the
//     re-acquire fires a second doJoinThing - the joinClient claim dedups it.
//     (destroyPromises separately serializes a rejoin behind a dying room's
//     destroy, the other half of the same dedup hazard.)
//   - ToC room: always-on, so no ref-count; `tocJoinClient` records the socket a
//     join was claimed for and a duplicate doJoinToc on it bails (see doJoinToc).
//   - Any future room (presence, AgentConversation, ...) must be idempotent too.

import { TOC_CONTAINER } from "@familiar-systems/types-campaign";
import type { ThingId } from "@familiar-systems/types-campaign";
import { LoroAdaptor } from "loro-adaptors/loro";
import type { LoroDoc as LoroDocType, TreeID } from "loro-crdt";
import { LoroDoc } from "loro-crdt";
import { LoroWebsocketClient, type LoroWebsocketClientRoom } from "loro-websocket";

import { moveTocNode as applyTocMove, readTocTree } from "../toc/toc-doc";
import type { TocTreeNode } from "../toc/toc-doc";

// Public, referentially-stable snapshot read by useSyncExternalStore. Doc-only:
// the editor-specific containerId is derived in the hook, keeping this transport
// layer free of an @familiar-systems/editor dependency.
export type ThingRoomSnapshot =
  | { status: "joining" }
  | { status: "joined"; doc: LoroDocType }
  | { status: "error"; message: string };

// Internal bookkeeping for one room. A ref-counted registry is inherently
// mutable; the discriminated union lives in the public `snapshot` field instead.
//
// No per-doc subscription is kept: the editor syncs through loro-prosemirror
// directly, and the public snapshot reference is stable across doc edits, so a
// doc-change subscription would never re-render a React consumer in this slice.
// A future live-derived view (e.g. the ToC) will add its own subscription.
interface ThingRoom {
  refCount: number;
  snapshot: ThingRoomSnapshot;
  room: LoroWebsocketClientRoom | null;
  leaveTimer: ReturnType<typeof setTimeout> | null;
  // The socket this room's join was claimed for. The ref-count guard above keeps
  // acquireThing idempotent within one socket, but a hard refresh churns the
  // provider (connect/close/connect) and close() clears `things`, so the
  // StrictMode re-acquire fires a second doJoinThing. This claim dedups it (see
  // the "Room joins must be idempotent per socket" note up top).
  joinClient: LoroWebsocketClient | null;
}

const LEAVE_DEBOUNCE_MS = 100;

// Single frozen instance so getThingState returns a stable reference for any
// not-yet-acquired room (otherwise useSyncExternalStore would loop forever).
const JOINING: ThingRoomSnapshot = Object.freeze({ status: "joining" });

// Public ToC snapshot read by useToc via useSyncExternalStore. The ToC room is
// always-on (joined on connect, torn down on close), so unlike thing rooms there
// is no acquire/release; the snapshot is a derived view of the live LoroTree.
export type TocSnapshot =
  | { status: "loading" }
  | { status: "ready"; tree: TocTreeNode[] }
  | { status: "error"; message: string };

const TOC_LOADING: TocSnapshot = Object.freeze({ status: "loading" });

function errMessage(err: unknown): string {
  return err instanceof Error ? err.message : "Failed to connect.";
}

export class LoroClientManager {
  private client: LoroWebsocketClient | null = null;

  // Gate that resolves once connect() has opened the socket. Load-bearing for
  // deep links: React fires child effects before parent effects, so reloading
  // directly on a thing URL runs useThingDoc's acquire (a doJoinThing) BEFORE
  // the provider's connect(). doJoinThing awaits this gate rather than failing.
  private connectGate: Promise<void> | null = null;
  private resolveConnect: (() => void) | null = null;

  private readonly things = new Map<ThingId, ThingRoom>();
  private readonly listeners = new Map<ThingId, Set<() => void>>();
  // In-flight room.destroy() per id. A re-join must wait for the prior destroy
  // so the underlying client doesn't dedup the new join against the dying room.
  private readonly destroyPromises = new Map<ThingId, Promise<void>>();

  // The always-on ToC room: one doc + room per manager lifetime, (re)joined on
  // connect and dropped on close. `tocUnsub` detaches the doc subscription that
  // recomputes the derived tree on every local commit or remote import.
  private tocDoc: LoroDocType | null = null;
  private tocRoom: LoroWebsocketClientRoom | null = null;
  private tocUnsub: (() => void) | null = null;
  private tocSnapshot: TocSnapshot = TOC_LOADING;
  private readonly tocListeners = new Set<() => void>();
  // The socket a ToC join has been claimed for: makes doJoinToc idempotent per
  // socket. See the "Room joins must be idempotent per socket" note up top.
  private tocJoinClient: LoroWebsocketClient | null = null;

  constructor(private readonly wsUrl: string) {
    // Pure: no side effects. The provider calls connect() in a useEffect.
  }

  // ---- connection lifecycle (provider-owned) ------------------------------

  /** Open the socket. Idempotent. */
  connect(): void {
    if (this.client) return;
    this.client = new LoroWebsocketClient({ url: this.wsUrl });
    this.resolveConnect?.();
    this.connectGate = null;
    this.resolveConnect = null;
    // The ToC room is always-on (the server's TocActor is an eager singleton), so
    // join it eagerly here rather than on a consumer's mount.
    void this.doJoinToc();
  }

  // ---- ToC room (useToc-owned, always-on) ---------------------------------

  /**
   * Join the campaign's "toc" room over this socket and start deriving the
   * reactive tree snapshot. Fire-and-forget from connect(); on a StrictMode
   * socket cycle the stale join detects the client swap after its awaits and
   * tears its own room down.
   */
  private async doJoinToc(): Promise<void> {
    await this.whenConnected();
    const client = this.client;
    if (!client) return; // closed before we could start
    // Idempotent per socket (see the header note): claim synchronously, before
    // any further await, so a duplicate StrictMode join on this same socket bails
    // here instead of colliding on client.join()'s dedup and clobbering the doc.
    if (this.tocJoinClient === client) return;
    this.tocJoinClient = client;

    const doc = new LoroDoc();
    try {
      await client.waitConnected();
      const room = await client.join({
        roomId: TOC_CONTAINER,
        crdtAdaptor: new LoroAdaptor(doc),
      });
      // Resolves once the server snapshot is applied, so the first paint has the
      // full tree rather than an empty one.
      await room.waitForReachingServerVersion();

      if (this.client !== client) {
        // Socket cycled under us; the fresh connect() runs its own ToC join.
        void room.destroy().catch(() => {});
        return;
      }

      this.tocRoom = room;
      this.tocDoc = doc;
      // Fires on local commits (our own moves) and remote imports (peer edits).
      this.tocUnsub = doc.subscribe(() => this.recomputeToc());
      this.recomputeToc();
    } catch (err) {
      if (this.client === client) {
        // Release the claim so a later connect() on this socket can retry.
        this.tocJoinClient = null;
        this.tocSnapshot = { status: "error", message: errMessage(err) };
        this.notifyToc();
      }
    }
  }

  /** Re-derive the immutable tree snapshot and wake subscribers. */
  private recomputeToc(): void {
    if (!this.tocDoc) return;
    const tree = readTocTree(this.tocDoc);
    this.tocSnapshot = { status: "ready", tree };
    this.notifyToc();
  }

  /** Close the socket and drop all room state. Idempotent. */
  close(): void {
    if (!this.client) return;
    // Tear down the always-on ToC room.
    this.tocUnsub?.();
    this.tocUnsub = null;
    if (this.tocRoom) {
      void this.tocRoom.destroy().catch(() => {});
      this.tocRoom = null;
    }
    this.tocDoc = null;
    this.tocSnapshot = TOC_LOADING;
    // Drop the per-socket join claim so the next connect() rejoins on its socket.
    this.tocJoinClient = null;
    for (const room of this.things.values()) {
      if (room.leaveTimer != null) clearTimeout(room.leaveTimer);
    }
    this.things.clear();
    this.destroyPromises.clear();
    this.client.close();
    this.client = null;
    // Re-arm the gate lazily (a fresh whenConnected() will create one) and wake
    // any hooks so they re-read JOINING / TOC_LOADING. Harmless if truly unmounting.
    for (const id of this.listeners.keys()) this.notify(id);
    this.notifyToc();
  }

  private whenConnected(): Promise<void> {
    if (this.client) return Promise.resolve();
    if (!this.connectGate) {
      this.connectGate = new Promise<void>((resolve) => {
        this.resolveConnect = resolve;
      });
    }
    return this.connectGate;
  }

  // ---- thing rooms (useThingDoc-owned) ------------------------------------

  /** Called from useThingDoc's mount effect. Idempotent; retries after error. */
  acquireThing(id: ThingId): void {
    const existing = this.things.get(id);
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
    this.things.set(id, {
      refCount,
      snapshot: JOINING,
      room: null,
      leaveTimer: null,
      joinClient: null,
    });
    this.notify(id);
    void this.doJoinThing(id);
  }

  /** Called from useThingDoc's cleanup. Debounced leave on the last release. */
  releaseThing(id: ThingId): void {
    const room = this.things.get(id);
    if (!room) return;
    room.refCount--;
    if (room.refCount > 0) return;

    switch (room.snapshot.status) {
      case "joined":
        room.leaveTimer = setTimeout(() => this.doLeaveThing(id), LEAVE_DEBOUNCE_MS);
        break;
      case "error":
        // Nothing joined to tear down; drop it so a future acquire is fresh.
        this.things.delete(id);
        break;
      case "joining":
        // The in-flight doJoinThing sees refCount <= 0 on completion and cleans up.
        break;
    }
  }

  private async doJoinThing(id: ThingId): Promise<void> {
    // Serialize behind any pending destroy of the same room (join dedup race).
    const pendingDestroy = this.destroyPromises.get(id);
    if (pendingDestroy) await pendingDestroy;

    await this.whenConnected();
    const client = this.client;
    if (!client) return; // closed before we could start

    const wanted = this.things.get(id);
    if (!wanted || wanted.refCount <= 0) {
      this.things.delete(id);
      return;
    }
    // Idempotent per socket (see the header note): on F5 close() clears `things`
    // between StrictMode's two acquires, so both fire a doJoinThing that resumes
    // against the same surviving socket. Claim synchronously, before any further
    // await, so the duplicate bails here instead of colliding on client.join()'s
    // dedup and binding the editor to a never-synced empty doc.
    if (wanted.joinClient === client) return;
    wanted.joinClient = client;

    const doc = new LoroDoc();
    try {
      await client.waitConnected();
      const room = await client.join({
        roomId: `thing:${id}`,
        crdtAdaptor: new LoroAdaptor(doc),
      });
      // Resolves once the server snapshot is applied locally, so the editor
      // mounts against fully-synced content (no empty-doc flash).
      await room.waitForReachingServerVersion();

      const current = this.things.get(id);
      if (!current || current.refCount <= 0 || this.client !== client) {
        // Released while joining, or the socket cycled under us (StrictMode
        // close+reconnect). Drop this room; a re-acquire joins on the new socket.
        void room.destroy().catch(() => {});
        if (current && current.refCount <= 0) this.things.delete(id);
        return;
      }

      current.room = room;
      current.snapshot = { status: "joined", doc };
      this.notify(id);
    } catch (err) {
      const current = this.things.get(id);
      if (current) {
        current.snapshot = { status: "error", message: errMessage(err) };
        this.notify(id);
      }
    }
  }

  private doLeaveThing(id: ThingId): void {
    const room = this.things.get(id);
    if (!room || room.snapshot.status !== "joined") return;
    this.things.delete(id);
    if (room.room) {
      const destroyPromise = room.room.destroy().catch(() => {});
      this.destroyPromises.set(id, destroyPromise);
      void destroyPromise.then(() => this.destroyPromises.delete(id));
    }
    this.notify(id);
  }

  // ---- useSyncExternalStore plumbing --------------------------------------

  subscribeThingDoc = (id: ThingId, listener: () => void): (() => void) => {
    let set = this.listeners.get(id);
    if (!set) {
      set = new Set();
      this.listeners.set(id, set);
    }
    set.add(listener);
    return () => {
      set.delete(listener);
      if (set.size === 0) this.listeners.delete(id);
    };
  };

  getThingState = (id: ThingId): ThingRoomSnapshot => {
    return this.things.get(id)?.snapshot ?? JOINING;
  };

  private notify(id: ThingId): void {
    this.listeners.get(id)?.forEach((l) => l());
  }

  // ---- ToC useSyncExternalStore plumbing + mutations ----------------------

  subscribeToc = (listener: () => void): (() => void) => {
    this.tocListeners.add(listener);
    return () => this.tocListeners.delete(listener);
  };

  getTocSnapshot = (): TocSnapshot => this.tocSnapshot;

  /**
   * Move a ToC node under `parent` (root when null) at sibling `index`. The local
   * commit syncs over the room and optimistically updates this snapshot through
   * the doc subscription. No-op until the ToC room has joined.
   */
  moveTocNode = (node: TreeID, parent: TreeID | null, index: number): void => {
    if (!this.tocDoc) return;
    applyTocMove(this.tocDoc, node, parent, index);
  };

  private notifyToc(): void {
    this.tocListeners.forEach((l) => l());
  }
}
