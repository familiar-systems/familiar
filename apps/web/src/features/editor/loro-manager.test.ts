// loro-websocket is not built for React's StrictMode (synchronous mount/unmount/
// remount) or for navigation that races joins against leaves. LoroClientManager is
// the container that consumes loro-websocket safely; these tests validate that
// container under StrictMode and the related race conditions.
//
// Vitest runs in the node environment here (no jsdom; see apps/web/vitest.config.ts
// and the sibling toc-doc.test.ts), so we drive the manager AS A CLASS with a
// mocked loro-websocket, not through a React render. The mock reproduces the one
// library behavior that made the original bug possible: client.join() dedups by
// (crdtType + roomId), and a deduped second call gets the FIRST join's room while
// its own doc is never wired (left empty). We simulate "server backfill" by
// writing a marker into the wired doc on a fresh join, so a test can assert the
// manager exposed the synced doc and not an empty loser.

import {
  TOC_CONTAINER,
  TOC_KEY_KIND,
  TOC_KEY_PAGE_ID,
  TOC_KEY_TITLE,
  TOC_KEY_VISIBILITY,
  TOC_KIND_PAGE,
  pageIdSchema,
} from "@familiar-systems/types-campaign";
import type { PageId } from "@familiar-systems/types-campaign";
import { LoroDoc } from "loro-crdt";
import type { LoroDoc as LoroDocType, TreeID } from "loro-crdt";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// A loro-adaptor-like shape: all the manager's mock path touches is crdtType and
// getDoc(). Kept narrow so the mock needs no loro-adaptors import.
interface AdaptorLike {
  crdtType: string;
  getDoc(): LoroDocType;
}

// The mock classes live inside vi.hoisted so vi.mock (which is hoisted above the
// imports) can reference them. The registry lets tests inspect what the manager
// did to the socket.
const mock = vi.hoisted(() => {
  interface Deferred<T> {
    promise: Promise<T>;
    resolve: (value: T) => void;
    reject: (reason: unknown) => void;
  }
  function deferred<T>(): Deferred<T> {
    let resolve!: (value: T) => void;
    let reject!: (reason: unknown) => void;
    const promise = new Promise<T>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  }

  interface JoinCall {
    roomId: string;
    adaptor: AdaptorLike;
    deduped: boolean;
  }

  class MockRoom {
    destroyed = false;
    // Controllable server-version handoff. Ungated rooms pre-resolve (the marker
    // write in join() already landed the "server snapshot", so the manager mounts
    // against synced content). A gated room stays pending so a test can resolve or
    // reject it LATE, modelling a join that completes after a teardown +
    // re-acquire has already replaced its handle (the join-identity race).
    readonly serverVersion = deferred<void>();
    constructor(
      readonly client: MockClient,
      readonly key: string,
      readonly roomId: string,
      readonly onStatusChange: ((s: string) => void) | undefined,
    ) {
      if (registry.gateNextRoom) {
        registry.gateNextRoom = false;
        registry.gatedRooms.push(this);
      } else {
        this.serverVersion.resolve();
      }
    }
    waitForReachingServerVersion(): Promise<void> {
      return this.serverVersion.promise;
    }
    leave(): Promise<void> {
      return Promise.resolve();
    }
    // Mirror the real cleanupRoom: a destroyed room leaves the dedup table, so a
    // later re-join builds a fresh room rather than colliding with a dead one.
    destroy(): Promise<void> {
      if (this.client.rooms.get(this.key) === this) this.client.rooms.delete(this.key);
      this.destroyed = true;
      return Promise.resolve();
    }
    fireStatus(s: string): void {
      this.onStatusChange?.(s);
    }
  }

  class MockClient {
    readonly rooms = new Map<string, MockRoom>();
    readonly joinCalls: JoinCall[] = [];
    closed = false;
    destroyed = false;
    constructor(readonly opts: { url: string }) {
      registry.instances.push(this);
    }
    waitConnected(): Promise<void> {
      return Promise.resolve();
    }
    connect(): Promise<void> {
      return Promise.resolve();
    }
    onStatusChange(): () => void {
      return () => {};
    }
    join(opts: {
      roomId: string;
      crdtAdaptor: AdaptorLike;
      onStatusChange?: (s: string) => void;
    }): Promise<MockRoom> {
      const key = opts.crdtAdaptor.crdtType + opts.roomId;
      const existing = this.rooms.get(key);
      this.joinCalls.push({
        roomId: opts.roomId,
        adaptor: opts.crdtAdaptor,
        deduped: existing != null,
      });
      if (existing) return Promise.resolve(existing);
      // Server backfill landing on the wired doc. A deduped second join skips
      // this, so its doc stays empty: that is the regression we guard.
      const doc = opts.crdtAdaptor.getDoc();
      doc.getMap("__synced__").set("ok", true);
      doc.commit();
      const room = new MockRoom(this, key, opts.roomId, opts.onStatusChange);
      this.rooms.set(key, room);
      return Promise.resolve(room);
    }
    close(): void {
      this.closed = true;
    }
    destroy(): void {
      this.destroyed = true;
    }
  }

  const registry = {
    instances: [] as MockClient[],
    // When set, the next room built by join() stays pending (gated) instead of
    // pre-resolving its server-version handoff. Consumed (reset to false) by the
    // room that claims it, so only one room is gated per arm.
    gateNextRoom: false,
    gatedRooms: [] as MockRoom[],
  };
  return {
    MockClient,
    instances: registry.instances,
    get gatedRooms(): MockRoom[] {
      return registry.gatedRooms;
    },
    armServerGate(): void {
      registry.gateNextRoom = true;
    },
    reset(): void {
      registry.instances.length = 0;
      registry.gatedRooms.length = 0;
      registry.gateNextRoom = false;
    },
  };
});

vi.mock("loro-websocket", () => ({ LoroWebsocketClient: mock.MockClient }));

// Imported after the mock is registered (the import itself is hoisted, but the
// manager only constructs a client on connect(), never at import time).
const { LoroClientManager } = await import("./loro-manager");

// LEAVE_DEBOUNCE_MS in the manager is 100; advance past it to fire a leave.
const PAST_DEBOUNCE_MS = 150;

const PAGE_A = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const PAGE_B = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

// Flush the manager's promise chains (waitConnected -> join ->
// waitForReachingServerVersion). Fake timers do not touch microtasks, so
// draining the microtask queue is enough; the debounced leave uses a real
// setTimeout and is advanced explicitly where a test needs it.
async function flushMicro(): Promise<void> {
  for (let i = 0; i < 12; i++) await Promise.resolve();
}

function makeManager(): InstanceType<typeof LoroClientManager> {
  return new LoroClientManager("ws://test/ws");
}

function addPageNode(doc: LoroDocType, title: string, pageId: string): void {
  const tree = doc.getTree(TOC_CONTAINER);
  if (!tree.isFractionalIndexEnabled()) tree.enableFractionalIndex(0);
  const node = tree.createNode();
  node.data.set(TOC_KEY_KIND, TOC_KIND_PAGE);
  node.data.set(TOC_KEY_TITLE, title);
  node.data.set(TOC_KEY_PAGE_ID, pageId);
  node.data.set(TOC_KEY_VISIBILITY, "gmOnly");
}

// The doc the manager bound for a Page room, asserted joined + synced. A Page
// room's view is the LoroDoc itself.
function expectSyncedPage(
  manager: InstanceType<typeof LoroClientManager>,
  id: PageId,
): LoroDocType {
  const state = manager.getPageState(id);
  expect(state.status).toBe("joined");
  if (state.status !== "joined") throw new Error("unreachable");
  expect(state.view.getMap("__synced__").get("ok")).toBe(true);
  return state.view;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  mock.reset();
  vi.clearAllMocks();
});

describe("LoroClientManager page rooms", () => {
  it("joins a Page room after connect and exposes the synced doc", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();

    expectSyncedPage(m, PAGE_A);
  });

  it("survives a StrictMode connect -> close -> connect cycle on one socket", async () => {
    // The documented hazard: a synchronous mount/cleanup/remount used to swap the
    // socket out from under the in-flight joins. Run it with no awaits between the
    // calls, then settle: the manager must converge on one joined room over a
    // single, never-torn-down client.
    const m = makeManager();
    m.acquirePage(PAGE_A); // child effect fires before the provider's connect
    m.connect();
    m.releasePage(PAGE_A);
    m.close();
    m.acquirePage(PAGE_A);
    m.connect();
    await flushMicro();

    expectSyncedPage(m, PAGE_A);
    expect(mock.instances).toHaveLength(1); // the debounced teardown was cancelled
    expect(mock.instances[0]?.destroyed).toBe(false);
  });

  it("never exposes an empty doc from a deduped join (regression canary)", async () => {
    // The canary for the original empty-view bug: after the strict cycle both
    // resumed joins target the surviving socket and collide on the library dedup;
    // the manager must expose the WIRED (non-empty) doc, never the deduped loser's.
    const m = makeManager();
    m.acquirePage(PAGE_A);
    m.connect();
    m.releasePage(PAGE_A);
    m.close();
    m.acquirePage(PAGE_A);
    m.connect();
    await flushMicro();

    const doc = expectSyncedPage(m, PAGE_A);
    expect(doc.getMap("__synced__").get("ok")).toBe(true);
  });

  it("deep link: acquire before connect reaches joined (lazy socket)", async () => {
    // React fires child effects before parent effects, so a reload straight onto a
    // Page URL runs acquirePage before the provider's connect(). The acquire
    // lazily opens the socket, so the join completes without waiting for connect().
    const m = makeManager();
    m.acquirePage(PAGE_A);
    await flushMicro();
    expectSyncedPage(m, PAGE_A);

    // A later connect() is idempotent and reuses the same socket.
    m.connect();
    await flushMicro();
    expectSyncedPage(m, PAGE_A);
    expect(mock.instances).toHaveLength(1);
  });

  it("coalesces a release immediately followed by a re-acquire (debounced leave)", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const doc = expectSyncedPage(m, PAGE_A);

    m.releasePage(PAGE_A); // schedules a debounced leave
    m.acquirePage(PAGE_A); // cancels it before it fires
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    await flushMicro();

    const after = expectSyncedPage(m, PAGE_A);
    expect(after).toBe(doc); // same doc retained, no teardown
    const room = mock.instances.at(-1)?.rooms.get("%LOR" + "page:" + PAGE_A);
    expect(room?.destroyed).toBe(false);
  });

  it("tears the room down when a release is not followed by a re-acquire", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const room = mock.instances.at(-1)?.rooms.get("%LOR" + "page:" + PAGE_A);

    m.releasePage(PAGE_A);
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    await flushMicro();

    expect(room?.destroyed).toBe(true);
    expect(m.getPageState(PAGE_A).status).toBe("joining"); // back to the default
  });

  it("tears the socket down after close() when no reconnect follows", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    expectSyncedPage(m, PAGE_A);

    m.close(); // genuine unmount: no connect() follows to cancel the teardown
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    await flushMicro();

    expect(mock.instances[0]?.destroyed).toBe(true);
    expect(m.getPageState(PAGE_A).status).toBe("joining"); // reset to default
  });

  it("surfaces a reconnecting status without dropping the doc", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const doc = expectSyncedPage(m, PAGE_A);

    const room = mock.instances.at(-1)?.rooms.get("%LOR" + "page:" + PAGE_A);
    room?.fireStatus("reconnecting");
    const reconnecting = m.getPageState(PAGE_A);
    expect(reconnecting.status).toBe("reconnecting");
    if (reconnecting.status === "reconnecting") expect(reconnecting.view).toBe(doc);

    room?.fireStatus("joined");
    const recovered = m.getPageState(PAGE_A);
    expect(recovered.status).toBe("joined");
    if (recovered.status === "joined") expect(recovered.view).toBe(doc); // same doc throughout
  });

  // A join is asynchronous, so the handle it was started for can be superseded
  // before it completes: a debounced teardown clears the rooms map and a
  // re-acquire installs a fresh handle (its own doc) under the same roomId. A
  // join that finishes after that must commit its room only to the handle that
  // requested it, and discard the room otherwise - binding it onto the newer
  // handle would leave that handle's own doc orphaned and rendering empty.
  // refCount cannot distinguish the two handles (both have refCount > 0); handle
  // identity can.
  it("a late-resolving stale join does not hijack a re-acquired handle", async () => {
    const m = makeManager();
    m.connect();
    mock.armServerGate(); // freeze this join at its server-version handoff
    m.acquirePage(PAGE_A); // handle A; join A suspends mid-flight
    await flushMicro();
    const roomA = mock.gatedRooms[0];
    expect(roomA).toBeDefined();

    m.close(); // debounced teardown clears the map under the in-flight join A
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    m.connect(); // remount: a fresh socket
    m.acquirePage(PAGE_A); // handle B, a NEW doc for the same room
    await flushMicro();
    expectSyncedPage(m, PAGE_A); // B joined on its own room/doc

    roomA?.serverVersion.resolve(); // join A finally resolves, too late
    await flushMicro();

    // The guard recognises A's room as stale and destroys it instead of binding
    // it onto B. Without it, A's room (wired to A's doc) would overwrite B's, and
    // B's own doc would be orphaned -> the editor renders an empty doc forever.
    expect(roomA?.destroyed).toBe(true);
    expectSyncedPage(m, PAGE_A); // B is still the joined, synced room
  });

  it("a late-rejecting stale join does not stamp error onto a re-acquired handle", async () => {
    const m = makeManager();
    m.connect();
    mock.armServerGate();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const roomA = mock.gatedRooms[0];
    expect(roomA).toBeDefined();

    m.close();
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    expectSyncedPage(m, PAGE_A);

    roomA?.serverVersion.reject(new Error("stale join failed"));
    await flushMicro();

    // The catch path carries the same identity guard: a stale rejection must not
    // flip the handle that replaced us into the error state.
    expect(m.getPageState(PAGE_A).status).toBe("joined");
  });
});

describe("LoroClientManager ToC room", () => {
  it("joins the ToC room on acquire and exposes the derived tree", async () => {
    // The ToC is an ordinary ref-counted room now (the layout pins it via
    // acquireToc); it is not auto-joined on connect.
    const m = makeManager();
    m.connect();
    m.acquireToc();
    await flushMicro();

    const snap = m.getTocSnapshot();
    expect(snap.status).toBe("joined");
    if (snap.status !== "joined") throw new Error("unreachable");
    expect(snap.view).toHaveLength(0);
  });

  it("recomputes the tree view on a doc commit", async () => {
    const m = makeManager();
    m.connect();
    m.acquireToc();
    await flushMicro();

    // Write into the same doc the manager subscribed to (the ToC adaptor's doc),
    // simulating a peer edit arriving over the room.
    const tocJoin = mock.instances.at(-1)?.joinCalls.find((c) => c.roomId === TOC_CONTAINER);
    expect(tocJoin).toBeDefined();
    const tocDoc = tocJoin?.adaptor.getDoc();
    if (!tocDoc) throw new Error("no toc doc");
    addPageNode(tocDoc, "Korgath", PAGE_B);
    tocDoc.commit();
    await flushMicro();

    const snap = m.getTocSnapshot();
    expect(snap.status).toBe("joined");
    if (snap.status !== "joined") throw new Error("unreachable");
    expect(snap.view).toHaveLength(1);
    expect(snap.view[0]?.entry).toMatchObject({ kind: "page", title: "Korgath" });
  });

  it("moveTocNode is a no-op before the ToC room has joined", () => {
    const m = makeManager();
    // A valid TreeID from a throwaway doc; the call should bail on the null doc
    // before ever touching it.
    const scratch = new LoroDoc();
    const tree = scratch.getTree("x");
    tree.enableFractionalIndex(0);
    const id: TreeID = tree.createNode().id;

    expect(() => m.moveTocNode(id, null, 0)).not.toThrow();
  });
});

// A terminally-errored room must release its websocket room + doc subscription and
// fail closed, never linger `bound` behind an error snapshot (where a later
// release/acquire drops the handle and leaks both). loro-websocket signals terminal
// two ways: `error`, and `disconnected` once it has given up reconnecting; both land
// here. These tests pin that teardown.
describe("LoroClientManager terminal room errors", () => {
  const pageKey = "%LOR" + "page:" + PAGE_A;

  it("tears the room down when a bound room terminally errors", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    expectSyncedPage(m, PAGE_A);
    const room = mock.instances.at(-1)?.rooms.get(pageKey);
    expect(room?.destroyed).toBe(false);

    room?.fireStatus("error");

    expect(room?.destroyed).toBe(true);
    const snap = m.getPageState(PAGE_A);
    expect(snap.status).toBe("error");
    if (snap.status === "error") expect(snap.error.kind).toBe("connection_lost");
  });

  it("treats a terminal disconnected like an error, not a perpetual reconnect", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const room = mock.instances.at(-1)?.rooms.get(pageKey);

    room?.fireStatus("disconnected");

    expect(room?.destroyed).toBe(true);
    const snap = m.getPageState(PAGE_A);
    expect(snap.status).toBe("error");
    if (snap.status === "error") expect(snap.error.kind).toBe("connection_lost");
  });

  it("a terminal error is final: a later status does not revive the room", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const room = mock.instances.at(-1)?.rooms.get(pageKey);
    room?.fireStatus("error");
    expect(m.getPageState(PAGE_A).status).toBe("error");

    room?.fireStatus("joined"); // binding is `failed` now, so onRoomStatus ignores it
    expect(m.getPageState(PAGE_A).status).toBe("error");
  });

  it("releasing an errored room drops the handle so the next acquire is fresh", async () => {
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const room = mock.instances.at(-1)?.rooms.get(pageKey);
    room?.fireStatus("error");

    m.releasePage(PAGE_A);
    expect(m.getPageState(PAGE_A).status).toBe("joining"); // handle gone

    m.acquirePage(PAGE_A);
    await flushMicro();
    expectSyncedPage(m, PAGE_A); // rebuilt on a fresh room + doc
    expect(room?.destroyed).toBe(true); // old room never leaked
  });

  it("re-acquiring over an errored room tears the old room down (no leak)", async () => {
    // A pinned room (the ToC) can't release, so recovery is a fresh acquire over the
    // errored handle. The overwrite must not strand the old room.
    const m = makeManager();
    m.connect();
    m.acquirePage(PAGE_A);
    await flushMicro();
    const first = mock.instances.at(-1)?.rooms.get(pageKey);
    first?.fireStatus("error");
    expect(first?.destroyed).toBe(true);

    m.acquirePage(PAGE_A);
    await flushMicro();
    expectSyncedPage(m, PAGE_A);
  });

  it("severs the doc subscription when a derived (ToC) room errors", async () => {
    // The derived ToC view subscribes to its doc; a terminal error must call that
    // docUnsub, not only destroy the room. LoroAdaptor subscribes via
    // subscribeLocalUpdates, so a spy on doc.subscribe catches only OUR derived sub.
    const realSubscribe = LoroDoc.prototype.subscribe;
    const unsubs: ReturnType<typeof vi.fn>[] = [];
    const spy = vi.spyOn(LoroDoc.prototype, "subscribe").mockImplementation(function (
      this: LoroDoc,
      ...args: Parameters<LoroDoc["subscribe"]>
    ) {
      const tracked = vi.fn(realSubscribe.apply(this, args));
      unsubs.push(tracked);
      return tracked;
    });
    try {
      const m = makeManager();
      m.connect();
      m.acquireToc();
      await flushMicro();
      expect(m.getTocSnapshot().status).toBe("joined");
      expect(unsubs).toHaveLength(1); // exactly the derived ToC subscription

      const tocRoom = mock.instances.at(-1)?.rooms.get("%LOR" + TOC_CONTAINER);
      tocRoom?.fireStatus("error");

      expect(tocRoom?.destroyed).toBe(true);
      expect(unsubs[0]).toHaveBeenCalledTimes(1); // docUnsub fired; no subscription leak
    } finally {
      spy.mockRestore();
    }
  });
});
