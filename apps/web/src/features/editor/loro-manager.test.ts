// Safety net for LoroClientManager's lifecycle: the StrictMode connect/close
// races and the empty-doc regression the last two commits fixed. Written first,
// against the CURRENT behavior, so the refactor that follows (stop the socket
// swap, then unify ToC + Thing into one room abstraction) is a refactor under
// green rather than a rewrite on faith.
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
  TOC_KEY_THING_ID,
  TOC_KEY_TITLE,
  TOC_KEY_VISIBILITY,
  TOC_KIND_THING,
  thingIdSchema,
} from "@familiar-systems/types-campaign";
import type { ThingId } from "@familiar-systems/types-campaign";
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
  interface JoinCall {
    roomId: string;
    adaptor: AdaptorLike;
    deduped: boolean;
  }

  class MockRoom {
    destroyed = false;
    constructor(
      readonly client: MockClient,
      readonly key: string,
      readonly roomId: string,
      readonly onStatusChange: ((s: string) => void) | undefined,
    ) {}
    // Resolves immediately: the marker write below already landed the "server
    // snapshot", so the manager mounts against synced content.
    waitForReachingServerVersion(): Promise<void> {
      return Promise.resolve();
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

  const registry = { instances: [] as MockClient[] };
  return {
    MockClient,
    instances: registry.instances,
    reset(): void {
      registry.instances.length = 0;
    },
  };
});

vi.mock("loro-websocket", () => ({ LoroWebsocketClient: mock.MockClient }));

// Imported after the mock is registered (the import itself is hoisted, but the
// manager only constructs a client on connect(), never at import time).
const { LoroClientManager } = await import("./loro-manager");

// LEAVE_DEBOUNCE_MS in the manager is 100; advance past it to fire a leave.
const PAST_DEBOUNCE_MS = 150;

const THING_A = thingIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const THING_B = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

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

function addThingNode(doc: LoroDocType, title: string, thingId: string): void {
  const tree = doc.getTree(TOC_CONTAINER);
  if (!tree.isFractionalIndexEnabled()) tree.enableFractionalIndex(0);
  const node = tree.createNode();
  node.data.set(TOC_KEY_KIND, TOC_KIND_THING);
  node.data.set(TOC_KEY_TITLE, title);
  node.data.set(TOC_KEY_THING_ID, thingId);
  node.data.set(TOC_KEY_VISIBILITY, "gmOnly");
}

// The doc the manager bound for a Thing room, asserted joined + synced. A Thing
// room's view is the LoroDoc itself.
function expectSyncedThing(
  manager: InstanceType<typeof LoroClientManager>,
  id: ThingId,
): LoroDocType {
  const state = manager.getThingState(id);
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

describe("LoroClientManager thing rooms", () => {
  it("joins a Thing room after connect and exposes the synced doc", async () => {
    const m = makeManager();
    m.connect();
    m.acquireThing(THING_A);
    await flushMicro();

    expectSyncedThing(m, THING_A);
  });

  it("survives a StrictMode connect -> close -> connect cycle on one socket", async () => {
    // The documented hazard: a synchronous mount/cleanup/remount used to swap the
    // socket out from under the in-flight joins. Run it with no awaits between the
    // calls, then settle: the manager must converge on one joined room over a
    // single, never-torn-down client.
    const m = makeManager();
    m.acquireThing(THING_A); // child effect fires before the provider's connect
    m.connect();
    m.releaseThing(THING_A);
    m.close();
    m.acquireThing(THING_A);
    m.connect();
    await flushMicro();

    expectSyncedThing(m, THING_A);
    expect(mock.instances).toHaveLength(1); // the debounced teardown was cancelled
    expect(mock.instances[0]?.destroyed).toBe(false);
  });

  it("never exposes an empty doc from a deduped join (regression canary)", async () => {
    // The canary for the original empty-view bug: after the strict cycle both
    // resumed joins target the surviving socket and collide on the library dedup;
    // the manager must expose the WIRED (non-empty) doc, never the deduped loser's.
    const m = makeManager();
    m.acquireThing(THING_A);
    m.connect();
    m.releaseThing(THING_A);
    m.close();
    m.acquireThing(THING_A);
    m.connect();
    await flushMicro();

    const doc = expectSyncedThing(m, THING_A);
    expect(doc.getMap("__synced__").get("ok")).toBe(true);
  });

  it("deep link: acquire before connect reaches joined (lazy socket)", async () => {
    // React fires child effects before parent effects, so a reload straight onto a
    // Thing URL runs acquireThing before the provider's connect(). The acquire
    // lazily opens the socket, so the join completes without waiting for connect().
    const m = makeManager();
    m.acquireThing(THING_A);
    await flushMicro();
    expectSyncedThing(m, THING_A);

    // A later connect() is idempotent and reuses the same socket.
    m.connect();
    await flushMicro();
    expectSyncedThing(m, THING_A);
    expect(mock.instances).toHaveLength(1);
  });

  it("coalesces a release immediately followed by a re-acquire (debounced leave)", async () => {
    const m = makeManager();
    m.connect();
    m.acquireThing(THING_A);
    await flushMicro();
    const doc = expectSyncedThing(m, THING_A);

    m.releaseThing(THING_A); // schedules a debounced leave
    m.acquireThing(THING_A); // cancels it before it fires
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    await flushMicro();

    const after = expectSyncedThing(m, THING_A);
    expect(after).toBe(doc); // same doc retained, no teardown
    const room = mock.instances.at(-1)?.rooms.get("%LOR" + "thing:" + THING_A);
    expect(room?.destroyed).toBe(false);
  });

  it("tears the room down when a release is not followed by a re-acquire", async () => {
    const m = makeManager();
    m.connect();
    m.acquireThing(THING_A);
    await flushMicro();
    const room = mock.instances.at(-1)?.rooms.get("%LOR" + "thing:" + THING_A);

    m.releaseThing(THING_A);
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    await flushMicro();

    expect(room?.destroyed).toBe(true);
    expect(m.getThingState(THING_A).status).toBe("joining"); // back to the default
  });

  it("tears the socket down after close() when no reconnect follows", async () => {
    const m = makeManager();
    m.connect();
    m.acquireThing(THING_A);
    await flushMicro();
    expectSyncedThing(m, THING_A);

    m.close(); // genuine unmount: no connect() follows to cancel the teardown
    await vi.advanceTimersByTimeAsync(PAST_DEBOUNCE_MS);
    await flushMicro();

    expect(mock.instances[0]?.destroyed).toBe(true);
    expect(m.getThingState(THING_A).status).toBe("joining"); // reset to default
  });

  it("surfaces a reconnecting status without dropping the doc", async () => {
    const m = makeManager();
    m.connect();
    m.acquireThing(THING_A);
    await flushMicro();
    const doc = expectSyncedThing(m, THING_A);

    const room = mock.instances.at(-1)?.rooms.get("%LOR" + "thing:" + THING_A);
    room?.fireStatus("reconnecting");
    const reconnecting = m.getThingState(THING_A);
    expect(reconnecting.status).toBe("reconnecting");
    if (reconnecting.status === "reconnecting") expect(reconnecting.view).toBe(doc);

    room?.fireStatus("joined");
    const recovered = m.getThingState(THING_A);
    expect(recovered.status).toBe("joined");
    if (recovered.status === "joined") expect(recovered.view).toBe(doc); // same doc throughout
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
    addThingNode(tocDoc, "Korgath", THING_B);
    tocDoc.commit();
    await flushMicro();

    const snap = m.getTocSnapshot();
    expect(snap.status).toBe("joined");
    if (snap.status !== "joined") throw new Error("unreachable");
    expect(snap.view).toHaveLength(1);
    expect(snap.view[0]?.entry).toMatchObject({ kind: "thing", title: "Korgath" });
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
