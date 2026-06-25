// Fetches a page's relationships from the campaign server and exposes a refetch.
// Plain useEffect + useState (this app has no React Query / global store; see
// apps/web/CLAUDE.md), mirroring the campaign index route's fetch shape. The
// response is already oriented to `pageId` server-side, so the view layer never
// computes edge direction. `refetch` is the seam Slices 5/6 call after a create
// or edit; read-only callers ignore it.

import type { CampaignId } from "@familiar-systems/types-app";
import type { PageId, RelationshipView } from "@familiar-systems/types-campaign";
import { useCallback, useEffect, useState } from "react";

import { campaignClient } from "../../lib/campaigns-api";

export type RelationshipsState =
  | { status: "loading" }
  | { status: "ready"; relationships: RelationshipView[] }
  | { status: "error"; message: string };

/**
 * Subscribe to `pageId`'s relationships. `enabled` is false for kinds that show a
 * static affordance instead of a list (templates), so no needless request fires.
 */
export function useRelationships(
  campaignId: CampaignId,
  pageId: PageId,
  enabled = true,
): { state: RelationshipsState; refetch: () => void } {
  const [state, setState] = useState<RelationshipsState>(
    enabled ? { status: "loading" } : { status: "ready", relationships: [] },
  );
  const [refetchKey, setRefetchKey] = useState(0);
  const refetch = useCallback(() => setRefetchKey((k) => k + 1), []);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    setState({ status: "loading" });
    void (async () => {
      const { data, response } = await campaignClient.GET(
        "/campaign/{id}/pages/{pageId}/relationships",
        { params: { path: { id: campaignId, pageId } } },
      );
      if (cancelled) return;
      if (!response.ok || data === undefined) {
        setState({ status: "error", message: `Failed to load relationships (${response.status})` });
        return;
      }
      // openapi-fetch types the response off the OpenAPI schema, whose branded
      // ids are a structural copy of the ts-rs brand rather than the `string &
      // {...}` primitive, so the two RelationshipView types aren't mutually
      // assignable though they describe the same JSON. Coerce to the ts-rs type
      // at this boundary, as the other campaign fetches do (campaign index route).
      setState({ status: "ready", relationships: data as RelationshipView[] });
    })();
    return () => {
      cancelled = true;
    };
  }, [campaignId, pageId, enabled, refetchKey]);

  return { state, refetch };
}
