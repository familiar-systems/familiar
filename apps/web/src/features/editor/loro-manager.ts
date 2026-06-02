// LoroClientManager: one campaign-scoped WebSocket, many CRDT rooms multiplexed
// over it. This is the "Repo" pattern (from loro-extended / SchoolAI): room
// join/leave lives here, not in React effects, so the doc + socket lifecycle is
// decoupled from component mount cycles. React hooks are read-only subscriptions
// (see LoroManagerProvider's useThingDoc + useSyncExternalStore).
//
// Why this over per-mount ownership: a single socket means no WebSocket
// handshake + auth + full-snapshot re-pull on every navigation, and it's the
// connection the imminent ToC room will share with thing rooms. This slice wires
// only the thing-room path; the ToC room lands with its consumer + shared schema.
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

import type { ThingId } from "@familiar-systems/types-campaign";
import { LoroAdaptor } from "loro-adaptors/loro";
import type { LoroDoc as LoroDocType } from "loro-crdt";
import { LoroDoc } from "loro-crdt";
import { LoroWebsocketClient, type LoroWebsocketClientRoom } from "loro-websocket";

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
}

const LEAVE_DEBOUNCE_MS = 100;

// Single frozen instance so getThingState returns a stable reference for any
// not-yet-acquired room (otherwise useSyncExternalStore would loop forever).
const JOINING: ThingRoomSnapshot = Object.freeze({ status: "joining" });

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
  }

  // TODO: (toc) join the always-on "toc" room here over this same socket and
  // expose a reactive ToC snapshot (subscribe/getSnapshot), deriving entries from
  // the shared ToC schema (campaign-shared / types-campaign), not hand-rolled
  // string keys. Lands next with the ToC consumer + new-page creation. Bring the
  // end-to-end test then too: doc reuse across navigation, refcount + debounced
  // leave, and one active socket held through navigation -- the machinery this
  // slice ships untested because there is only one room today.

  /** Close the socket and drop all room state. Idempotent. */
  close(): void {
    if (!this.client) return;
    for (const room of this.things.values()) {
      if (room.leaveTimer != null) clearTimeout(room.leaveTimer);
    }
    this.things.clear();
    this.destroyPromises.clear();
    this.client.close();
    this.client = null;
    // Re-arm the gate lazily (a fresh whenConnected() will create one) and wake
    // any hooks so they re-read JOINING. Harmless if we're truly unmounting.
    for (const id of this.listeners.keys()) this.notify(id);
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
}
