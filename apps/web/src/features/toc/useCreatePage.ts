// Create a new page over the typed campaign API, then navigate to its editor.
// Creation is a REST call on purpose: the server spawns the owning PageActor
// (genesis) and atomically inserts the ToC node, so the new entry arrives in the
// sidebar over the "toc" room on its own.
//
// Sessions and entities are different routes with different response shapes, so
// the create call is dispatched on the PageKind. A session may be created
// unnamed (the server titles it "Untitled Session"); an entity must be named.

import type { CampaignId } from "@familiar-systems/types-app";
import { pageIdSchema } from "@familiar-systems/types-campaign";
import type { PageId, PageKind } from "@familiar-systems/types-campaign";
import { useNavigate } from "@tanstack/react-router";
import { useCallback } from "react";

import { campaignClient } from "../../lib/campaigns-api";

export type CreatePage = (
  kind: PageKind,
  name: string | null,
  parent: PageId | null,
) => Promise<void>;

export function useCreatePage(campaignId: CampaignId): CreatePage {
  const navigate = useNavigate();
  return useCallback<CreatePage>(
    async (kind, name, parent) => {
      const pageId = await createByKind(campaignId, kind, name, parent);
      // Re-brand the response id through the schema (validate-at-the-boundary).
      // This also sidesteps openapi-fetch widening PageId into a structural
      // object form the router's PageId param no longer accepts.
      await navigate({
        to: "/c/$campaignId/p/$pageId",
        params: { campaignId, pageId },
      });
    },
    [campaignId, navigate],
  );
}

// Dispatch over PageKind. Exhaustive: a new variant trips the `never` arm at
// compile time, mirroring the Rust `match`. Each kind hits its own route and
// reads the new page id from a different response field.
async function createByKind(
  campaignId: CampaignId,
  kind: PageKind,
  name: string | null,
  parent: PageId | null,
): Promise<PageId> {
  switch (kind) {
    case "session": {
      const { data, response } = await campaignClient.POST("/campaign/{id}/sessions", {
        params: { path: { id: campaignId } },
        // Status omitted (null) defaults to gm_only server-side. A null/blank
        // name becomes "Untitled Session" on the server.
        body: { name, status: null, parent },
      });
      if (!response.ok || data === undefined) {
        throw new Error(`Failed to create session (${response.status}).`);
      }
      return pageIdSchema.parse(data.page_id);
    }
    case "entity": {
      const { data, response } = await campaignClient.POST("/campaign/{id}/pages", {
        params: { path: { id: campaignId } },
        // Entity names are required; the modal gates an empty submit, so `name`
        // is a real string here. Templates unused (from_template_id null).
        body: { name: name ?? "", status: null, parent, from_template_id: null },
      });
      if (!response.ok || data === undefined) {
        throw new Error(`Failed to create page (${response.status}).`);
      }
      return pageIdSchema.parse(data.id);
    }
    case "template":
      // No menu row offers this; template instantiation is unbuilt (server 501).
      throw new Error("Template creation is not built yet.");
    default: {
      const _exhaustive: never = kind;
      throw new Error(`Unhandled PageKind: ${String(_exhaustive)}`);
    }
  }
}
