// Create a new page (Page) over the typed campaign API, then navigate to its
// editor. Creation is a REST call on purpose: the server spawns the owning
// PageActor (genesis) and atomically inserts the ToC node via AddPageNode, so
// the new entry arrives in the sidebar over the "toc" room on its own. The name
// is required at creation because in-editor title editing does not exist yet.

import type { CampaignId } from "@familiar-systems/types-app";
import { pageIdSchema } from "@familiar-systems/types-campaign";
import type { PageId } from "@familiar-systems/types-campaign";
import { useNavigate } from "@tanstack/react-router";
import { useCallback } from "react";

import { campaignClient } from "../../lib/campaigns-api";

export type CreatePage = (name: string, parent: PageId | null) => Promise<void>;

export function useCreatePage(campaignId: CampaignId): CreatePage {
  const navigate = useNavigate();
  return useCallback<CreatePage>(
    async (name, parent) => {
      const { data, response } = await campaignClient.POST("/campaign/{id}/pages", {
        params: { path: { id: campaignId } },
        // Status omitted (null) defaults to gm_only server-side. Templates unused.
        body: { name, status: null, parent, from_template_id: null },
      });
      if (!response.ok || data === undefined) {
        throw new Error(`Failed to create page (${response.status}).`);
      }
      // Re-brand the response id through the schema (validate-at-the-boundary).
      // This also sidesteps openapi-fetch expanding PageId into a structural
      // object form that the router's PageId param no longer accepts.
      await navigate({
        to: "/c/$campaignId/p/$pageId",
        params: { campaignId, pageId: pageIdSchema.parse(data.id) },
      });
    },
    [campaignId, navigate],
  );
}
