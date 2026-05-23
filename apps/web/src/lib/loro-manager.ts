/**
 * LoroClientManager: singleton per campaign that owns all CRDT document
 * lifecycle.
 *
 * Repo pattern (validated in the prototype):
 * - One LoroWebsocketClient (single WebSocket, room multiplexing)
 * - ToC room joined eagerly, never left (always visible in sidebar)
 * - Page rooms acquired/released with debounced leave
 * - useSyncExternalStore-compatible subscribe/getSnapshot API
 *
 * Production uses LoroTree (not LoroMovableList like the prototype) for
 * the ToC. Each tree node's metadata map carries kind, title, thingId.
 */

import { LoroWebsocketClient } from "loro-websocket";
import { LoroAdaptor } from "loro-adaptors/loro";
import { LoroDoc } from "loro-crdt";
import type { LoroWebsocketClientRoom } from "loro-websocket";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface TocTreeEntry {
  treeId: string;
  kind: string;
  title: string;
  thingId?: string | undefined;
  children: TocTreeEntry[];
}

export type TocSnapshot =
  | { status: "loading"; entries: readonly TocTreeEntry[]; landingPageId: null }
  | {
      status: "ready";
      entries: readonly TocTreeEntry[];
      landingPageId: string | null;
    };

type TocRoomState =
  | { status: "joining" }
  | {
      status: "joined";
      doc: LoroDoc;
      adaptor: LoroAdaptor;
      room: LoroWebsocketClientRoom;
    };

type PageRoomState =
  | { status: "joining"; refCount: number; promise: Promise<LoroDoc> }
  | {
      status: "joined";
      refCount: number;
      doc: LoroDoc;
      adaptor: LoroAdaptor;
      room: LoroWebsocketClientRoom;
      leaveTimer: ReturnType<typeof setTimeout> | null;
    };

const LEAVE_DEBOUNCE_MS = 100;

const LOADING_SNAPSHOT: TocSnapshot = Object.freeze({
  status: "loading" as const,
  entries: Object.freeze([]) as readonly TocTreeEntry[],
  landingPageId: null,
});

// ---------------------------------------------------------------------------
// LoroClientManager
// ---------------------------------------------------------------------------

export class LoroClientManager {
  private client: LoroWebsocketClient | null = null;
  private tocState: TocRoomState = { status: "joining" };
  private cachedTocSnapshot: TocSnapshot | null = LOADING_SNAPSHOT;
  private readonly tocListeners = new Set<() => void>();
  private readonly pageStates = new Map<string, PageRoomState>();
  private readonly pageListeners = new Map<string, Set<() => void>>();
  private readonly destroyPromises = new Map<string, Promise<void>>();

  constructor(private readonly wsUrl: string) {}

  connect(): void {
    if (this.client) return;
    this.client = new LoroWebsocketClient({ url: this.wsUrl });
    void this.joinToc();
  }

  close(): void {
    if (!this.client) return;
    this.client.close();
    this.client = null;
    this.tocState = { status: "joining" };
    this.cachedTocSnapshot = LOADING_SNAPSHOT;
    this.notifyTocListeners();
    for (const [, state] of this.pageStates) {
      if (state.status === "joined" && state.leaveTimer) {
        clearTimeout(state.leaveTimer);
      }
    }
    this.pageStates.clear();
  }

  // ---- ToC room (eagerly joined, never left) ----

  private async joinToc(): Promise<void> {
    if (!this.client) return;
    try {
      const doc = new LoroDoc();
      const adaptor = new LoroAdaptor(doc);
      const room = await this.client.join({
        roomId: "toc",
        crdtAdaptor: adaptor,
      });
      await adaptor.waitForReachingServerVersion();

      this.tocState = { status: "joined", doc, adaptor, room };

      doc.subscribe(() => {
        this.cachedTocSnapshot = null;
        this.notifyTocListeners();
      });

      this.cachedTocSnapshot = null;
      this.notifyTocListeners();
    } catch (err) {
      console.error("[LoroClientManager] Failed to join ToC room:", err);
    }
  }

  get tocDoc(): LoroDoc | null {
    return this.tocState.status === "joined" ? this.tocState.doc : null;
  }

  subscribeToc = (listener: () => void): (() => void) => {
    this.tocListeners.add(listener);
    return () => {
      this.tocListeners.delete(listener);
    };
  };

  getTocSnapshot = (): TocSnapshot => {
    if (this.cachedTocSnapshot != null) return this.cachedTocSnapshot;

    if (this.tocState.status !== "joined") {
      this.cachedTocSnapshot = LOADING_SNAPSHOT;
      return this.cachedTocSnapshot;
    }

    this.cachedTocSnapshot = {
      status: "ready",
      entries: deriveTocTree(this.tocState.doc),
      landingPageId: deriveLandingPageId(this.tocState.doc),
    };
    return this.cachedTocSnapshot;
  };

  private notifyTocListeners(): void {
    this.tocListeners.forEach((l) => l());
  }

  // ---- Page rooms (acquire/release with debounced leave) ----

  acquirePage(pageId: string): void {
    const existing = this.pageStates.get(pageId);

    if (existing?.status === "joined") {
      existing.refCount++;
      if (existing.leaveTimer != null) {
        clearTimeout(existing.leaveTimer);
        existing.leaveTimer = null;
      }
      return;
    }

    if (existing?.status === "joining") {
      existing.refCount++;
      return;
    }

    const promise = this.doJoinPage(pageId);
    this.pageStates.set(pageId, { status: "joining", refCount: 1, promise });
  }

  releasePage(pageId: string): void {
    const state = this.pageStates.get(pageId);
    if (!state) return;

    state.refCount--;
    if (state.refCount > 0) return;

    if (state.status === "joined") {
      state.leaveTimer = setTimeout(() => {
        this.doLeavePage(pageId);
      }, LEAVE_DEBOUNCE_MS);
    }
  }

  private async doJoinPage(pageId: string): Promise<LoroDoc> {
    if (!this.client) throw new Error("Not connected");
    try {
      const pendingDestroy = this.destroyPromises.get(pageId);
      if (pendingDestroy) {
        await pendingDestroy;
      }

      const doc = new LoroDoc();
      const adaptor = new LoroAdaptor(doc);

      const room = await this.client.join({
        roomId: pageId,
        crdtAdaptor: adaptor,
      });
      await adaptor.waitForReachingServerVersion();

      const current = this.pageStates.get(pageId);
      if (!current || current.refCount <= 0) {
        room.destroy().catch(() => {});
        this.pageStates.delete(pageId);
        return doc;
      }

      this.pageStates.set(pageId, {
        status: "joined",
        refCount: current.refCount,
        doc,
        adaptor,
        room,
        leaveTimer: null,
      });

      doc.subscribe(() => {
        this.notifyPageListeners(pageId);
      });

      this.notifyPageListeners(pageId);
      return doc;
    } catch (err) {
      console.error(
        `[LoroClientManager] Failed to join page room ${pageId}:`,
        err,
      );
      this.pageStates.delete(pageId);
      this.notifyPageListeners(pageId);
      throw err;
    }
  }

  private doLeavePage(pageId: string): void {
    const state = this.pageStates.get(pageId);
    if (!state || state.status !== "joined") return;

    this.pageStates.delete(pageId);
    const destroyPromise = state.room.destroy().catch(() => {});
    this.destroyPromises.set(pageId, destroyPromise);
    void destroyPromise.then(() => {
      this.destroyPromises.delete(pageId);
    });
    this.notifyPageListeners(pageId);
  }

  subscribePageDoc = (
    pageId: string,
    listener: () => void,
  ): (() => void) => {
    let listeners = this.pageListeners.get(pageId);
    if (!listeners) {
      listeners = new Set();
      this.pageListeners.set(pageId, listeners);
    }
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0) {
        this.pageListeners.delete(pageId);
      }
    };
  };

  getPageDoc = (pageId: string): LoroDoc | null => {
    const state = this.pageStates.get(pageId);
    if (state?.status === "joined") return state.doc;
    return null;
  };

  private notifyPageListeners(pageId: string): void {
    this.pageListeners.get(pageId)?.forEach((l) => l());
  }
}

// ---------------------------------------------------------------------------
// ToC tree derivation (LoroTree, not LoroMovableList)
// ---------------------------------------------------------------------------

function deriveTocTree(doc: LoroDoc): TocTreeEntry[] {
  try {
    const tree = doc.getTree("toc");
    return buildNodeList(tree.toArray());
  } catch {
    return [];
  }
}

function buildNodeList(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  nodes: any[],
): TocTreeEntry[] {
  const entries: TocTreeEntry[] = [];
  for (const node of nodes) {
    const meta = node.data;
    if (!meta || typeof meta !== "object") continue;

    const getData = (key: string): unknown =>
      typeof meta.get === "function" ? meta.get(key) : undefined;

    const kind = String(getData("kind") ?? "text");
    const title = String(getData("title") ?? "");
    const rawThingId = getData("thingId");
    const thingId =
      typeof rawThingId === "string" && rawThingId.length > 0
        ? rawThingId
        : undefined;

    const childNodes = typeof node.children === "function" ? node.children() : [];
    const children = buildNodeList(childNodes);

    entries.push({
      treeId: String(node.id),
      kind,
      title,
      thingId,
      children,
    });
  }
  return entries;
}

function deriveLandingPageId(doc: LoroDoc): string | null {
  try {
    const meta = doc.getMap("meta");
    const val = meta.get("landingPageId");
    return typeof val === "string" && val.length > 0 ? val : null;
  } catch {
    return null;
  }
}
