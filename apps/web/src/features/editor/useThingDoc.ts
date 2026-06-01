// Opens a CRDT-synced Loro document for one Thing over the campaign server's
// WebSocket. Owns the LoroDoc + connection lifecycle; the editor binding
// (loro-prosemirror) lives in @familiar-systems/editor, the transport here.
//
// The official loro-protocol TS client (loro-websocket + loro-adaptors) speaks
// the same wire protocol (v0.3.0) as the Rust server, so joining a room and
// auto-syncing edits is just `client.join({ roomId, crdtAdaptor })`.

import { contentContainerId } from "@familiar-systems/editor";
import type { ThingId } from "@familiar-systems/types-campaign";
import { LoroAdaptor } from "loro-adaptors/loro";
import type { ContainerID, LoroDoc as LoroDocType } from "loro-crdt";
import { LoroDoc } from "loro-crdt";
import { LoroWebsocketClient, type LoroWebsocketClientRoom } from "loro-websocket";
import { useEffect, useState } from "react";

import { getSessionToken } from "../../lib/hanko";
import { wsUrl } from "../../lib/paths";

export type ThingDocState =
  | { status: "connecting" }
  | { status: "synced"; doc: LoroDocType; containerId: ContainerID }
  | { status: "error"; message: string };

export function useThingDoc(campaignId: string, thingId: ThingId): ThingDocState {
  const [state, setState] = useState<ThingDocState>({ status: "connecting" });

  useEffect(() => {
    setState({ status: "connecting" });

    const token = getSessionToken();
    if (token === null) {
      setState({ status: "error", message: "You are not signed in." });
      return;
    }

    // Each mount gets a fresh LoroDoc (and thus a fresh random PeerID -- never
    // reuse a PeerID across concurrent writers). StrictMode's double-mount is
    // handled by the `cancelled` guard plus synchronous teardown below.
    let cancelled = false;
    const doc = new LoroDoc();
    const client = new LoroWebsocketClient({
      url: wsUrl(`${campaignId}/ws?token=${encodeURIComponent(token)}`),
    });
    let room: LoroWebsocketClientRoom | null = null;

    void (async () => {
      try {
        await client.waitConnected();
        const joined = await client.join({
          roomId: `thing:${thingId}`,
          crdtAdaptor: new LoroAdaptor(doc),
        });
        if (cancelled) {
          await joined.destroy();
          return;
        }
        room = joined;
        // Resolves once the server's snapshot has been applied locally, so the
        // editor mounts against fully-populated content (no empty-doc flash).
        await joined.waitForReachingServerVersion();
        if (!cancelled) {
          setState({ status: "synced", doc, containerId: contentContainerId(doc) });
        }
      } catch (err) {
        if (!cancelled) {
          const message = err instanceof Error ? err.message : "Failed to connect.";
          setState({ status: "error", message });
        }
      }
    })();

    return () => {
      cancelled = true;
      void room?.destroy();
      client.close();
      client.destroy();
    };
  }, [campaignId, thingId]);

  return state;
}
