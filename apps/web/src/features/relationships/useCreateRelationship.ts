// Network glue for the create-relationship modal: fetches the predicate vocab and
// the session list once the flow is opened, and binds the three actions the modal
// needs (search entities, mint an entity, submit the relationship) to the typed
// campaign API. Keeping this out of the modal lets the modal stay presentational
// and play-testable with spied callbacks, mirroring useRelationships /
// RelationshipsWidget.
//
// `enabled` gates the one-time fetch so a closed modal costs nothing (same pattern
// as useRelationships' `enabled`). Until both fetches land, `sessions` is null and
// the connector holds the modal back (the as-of picker needs the session list).

import type { CampaignId } from "@familiar-systems/types-app";
import type {
  CreateRelationshipRequest,
  EntitySearchResult,
  PageId,
  PredicatePairView,
  SessionsResponse,
} from "@familiar-systems/types-campaign";
import { useCallback, useEffect, useRef, useState } from "react";

import { campaignClient } from "../../lib/campaigns-api";
import { createByKind } from "../toc/useCreatePage";
import { relationshipErrorMessage } from "./relationshipErrors";

// The object search runs on every keystroke; debounce it so typing doesn't fan out
// a request per character. The modal already guards against out-of-order results,
// so this only needs to throttle volume.
const SEARCH_DEBOUNCE_MS = 180;

export interface CreateRelationshipApi {
  predicates: PredicatePairView[];
  sessions: SessionsResponse | null;
  searchEntities: (query: string) => Promise<EntitySearchResult[]>;
  createEntity: (name: string) => Promise<{ id: PageId; name: string }>;
  submit: (req: CreateRelationshipRequest) => Promise<void>;
}

export function useCreateRelationship(
  campaignId: CampaignId,
  enabled: boolean,
): CreateRelationshipApi {
  const [predicates, setPredicates] = useState<PredicatePairView[]>([]);
  const [sessions, setSessions] = useState<SessionsResponse | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void (async () => {
      const [predRes, sessRes] = await Promise.all([
        campaignClient.GET("/campaign/{id}/relationships/predicates", {
          params: { path: { id: campaignId } },
        }),
        campaignClient.GET("/campaign/{id}/sessions", { params: { path: { id: campaignId } } }),
      ]);
      if (cancelled) return;
      // Coerce to the ts-rs types at this boundary, as the other campaign fetches
      // do: openapi-fetch types the response off a structural copy of the brand.
      if (predRes.response.ok && predRes.data !== undefined) {
        setPredicates(predRes.data as PredicatePairView[]);
      }
      if (sessRes.response.ok && sessRes.data !== undefined) {
        setSessions(sessRes.data as SessionsResponse);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [campaignId, enabled]);

  // Hold the in-flight debounce so a newer query cancels and settles the prior one
  // (resolving it empty rather than leaving a dangling promise the modal awaits).
  const pendingRef = useRef<{
    timer: ReturnType<typeof setTimeout>;
    resolve: (r: EntitySearchResult[]) => void;
  } | null>(null);

  const searchEntities = useCallback(
    (query: string): Promise<EntitySearchResult[]> =>
      new Promise<EntitySearchResult[]>((resolve) => {
        if (pendingRef.current !== null) {
          clearTimeout(pendingRef.current.timer);
          pendingRef.current.resolve([]);
        }
        const timer = setTimeout(() => {
          pendingRef.current = null;
          void (async () => {
            const { data, response } = await campaignClient.GET("/campaign/{id}/entities", {
              params: { path: { id: campaignId }, query: { q: query } },
            });
            resolve(response.ok && data !== undefined ? (data as EntitySearchResult[]) : []);
          })();
        }, SEARCH_DEBOUNCE_MS);
        pendingRef.current = { timer, resolve };
      }),
    [campaignId],
  );

  // Closing the modal mid-type unmounts this hook; clear any armed debounce so a
  // pending search can't fire (and resolve into an unmounted component) afterwards.
  useEffect(
    () => () => {
      if (pendingRef.current !== null) clearTimeout(pendingRef.current.timer);
    },
    [],
  );

  const createEntity = useCallback(
    async (name: string): Promise<{ id: PageId; name: string }> => {
      const id = await createByKind(campaignId, "entity", name, null);
      return { id, name };
    },
    [campaignId],
  );

  const submit = useCallback(
    async (req: CreateRelationshipRequest): Promise<void> => {
      const { response } = await campaignClient.POST("/campaign/{id}/relationships", {
        params: { path: { id: campaignId } },
        body: req,
      });
      if (!response.ok) throw new Error(messageForStatus(response.status));
    },
    [campaignId],
  );

  return { predicates, sessions, searchEntities, createEntity, submit };
}

function messageForStatus(status: number): string {
  return relationshipErrorMessage(status, {
    conflict: "A live relationship with this predicate pair already exists between these two.",
    unprocessable:
      "That relationship can't be created. It needs two different things and a predicate on each side.",
    notFound: "One of those pages no longer exists. Refresh and try again.",
    verb: "create",
  });
}
