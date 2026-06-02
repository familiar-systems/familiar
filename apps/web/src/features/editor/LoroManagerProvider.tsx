// React wiring for LoroClientManager. The manager owns the campaign WebSocket
// and all CRDT rooms; this provider just constructs it (purely) and drives its
// connect/close from a useEffect so React StrictMode's mount -> cleanup ->
// remount opens exactly one live socket. Mounted once per campaign at the
// /c/$campaignId layout (keyed by campaignId so switching campaigns rebuilds the
// manager against the new socket URL). Consumers read docs via useThingDoc.

import type { CampaignId } from "@familiar-systems/types-app";
import { createContext, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";

import { getSessionToken } from "../../lib/hanko";
import { wsUrl } from "../../lib/paths";
import { LoroClientManager } from "./loro-manager";

const LoroManagerContext = createContext<LoroClientManager | null>(null);

interface LoroManagerProviderProps {
  campaignId: CampaignId;
  children: ReactNode;
}

export function LoroManagerProvider({
  campaignId,
  children,
}: LoroManagerProviderProps): React.ReactElement {
  // Pure construction (no socket opened here). The token rides the WS query
  // string because the upgrade can't carry an Authorization header.
  const [manager] = useState(() => {
    // _authed guarantees a validated session before any /c route mounts, so the
    // token is present. A null here is session desync, not a normal state: fail
    // loudly rather than opening an unauthenticated socket (no silent fallback).
    const token = getSessionToken();
    if (token === null) {
      throw new Error("LoroManagerProvider mounted without a session token");
    }
    return new LoroClientManager(wsUrl(`${campaignId}/ws?token=${encodeURIComponent(token)}`));
  });

  useEffect(() => {
    manager.connect();
    return () => manager.close();
  }, [manager]);

  return <LoroManagerContext.Provider value={manager}>{children}</LoroManagerContext.Provider>;
}

export function useLoroManager(): LoroClientManager {
  const manager = useContext(LoroManagerContext);
  if (manager === null) {
    throw new Error("useLoroManager must be used within a LoroManagerProvider");
  }
  return manager;
}
