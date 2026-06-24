// Network glue for the edit-relationship modal. Lighter than useCreateRelationship:
// the edit flow needs only the session list (for the supersede / end as-of pickers),
// since the supersede panel is plain pre-filled inputs, not a predicate typeahead.
// `apply` routes the modal's assembled EditSubmit to the right verb: POST (supersede
// mints a new row + ends the old atomically), PATCH (end / retcon / visibility), or
// DELETE. Keeping this out of the modal keeps the modal presentational and
// play-testable with a spied callback, mirroring useCreateRelationship.
//
// `enabled` gates the one-time fetch so a closed modal costs nothing. Until the
// fetch lands, `sessions` is null and the connector holds the modal back (the
// as-of picker needs the session list).

import type { CampaignId } from "@familiar-systems/types-app";
import type { RelationshipId, SessionsResponse } from "@familiar-systems/types-campaign";
import { useCallback, useEffect, useState } from "react";

import { campaignClient } from "../../lib/campaigns-api";
import type { EditSubmit } from "./EditRelationshipModal";
import { relationshipErrorMessage } from "./relationshipErrors";

export interface EditRelationshipApi {
  sessions: SessionsResponse | null;
  apply: (relId: RelationshipId, submit: EditSubmit) => Promise<void>;
}

export function useEditRelationship(campaignId: CampaignId, enabled: boolean): EditRelationshipApi {
  const [sessions, setSessions] = useState<SessionsResponse | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void (async () => {
      const { data, response } = await campaignClient.GET("/campaign/{id}/sessions", {
        params: { path: { id: campaignId } },
      });
      if (cancelled) return;
      // Coerce to the ts-rs type at this boundary, as the other campaign fetches do:
      // openapi-fetch types the response off a structural copy of the brand.
      if (response.ok && data !== undefined) setSessions(data as SessionsResponse);
    })();
    return () => {
      cancelled = true;
    };
  }, [campaignId, enabled]);

  const apply = useCallback(
    async (relId: RelationshipId, submit: EditSubmit): Promise<void> => {
      switch (submit.kind) {
        case "supersede": {
          const { response } = await campaignClient.POST("/campaign/{id}/relationships", {
            params: { path: { id: campaignId } },
            body: submit.body,
          });
          if (!response.ok) throw new Error(messageForStatus(response.status));
          return;
        }
        case "patch": {
          const { response } = await campaignClient.PATCH("/campaign/{id}/relationships/{relId}", {
            params: { path: { id: campaignId, relId } },
            body: submit.body,
          });
          if (!response.ok) throw new Error(messageForStatus(response.status));
          return;
        }
        case "delete": {
          const { response } = await campaignClient.DELETE("/campaign/{id}/relationships/{relId}", {
            params: { path: { id: campaignId, relId } },
          });
          if (!response.ok) throw new Error(messageForStatus(response.status));
          return;
        }
      }
    },
    [campaignId],
  );

  return { sessions, apply };
}

function messageForStatus(status: number): string {
  return relationshipErrorMessage(status, {
    // 409 on edit: the row changed under the GM (already invalidated, or a supersede
    // whose new pair duplicates a live fact) - distinct from create's "already exists".
    conflict: "This relationship was already changed. Refresh and try again.",
    unprocessable: "That change isn't valid for this relationship.",
    notFound: "This relationship no longer exists. Refresh and try again.",
    verb: "update",
  });
}
