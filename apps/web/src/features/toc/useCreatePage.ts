// Create a new page over the typed campaign API, then navigate to its editor.
// Creation is a REST call on purpose: the server spawns the owning PageActor
// (genesis) and atomically inserts the ToC node, so the new entry arrives in the
// sidebar over the "toc" room on its own.
//
// One endpoint creates every kind: `POST /campaign/{id}/pages` takes a
// kind-tagged request (`{ kind, content }`) and returns a kind-tagged response.
// The PageKind selects the request variant (and, for a session, the response
// reports the new page via `page_id` instead of `id`).

import type { CampaignId } from "@familiar-systems/types-app";
import { pageIdSchema } from "@familiar-systems/types-campaign";
import type { CreatePageRequest, PageId, PageKind } from "@familiar-systems/types-campaign";
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
      await navigate({
        to: "/c/$campaignId/p/$pageId",
        params: { campaignId, pageId },
      });
    },
    [campaignId, navigate],
  );
}

// Build the kind's request variant, POST it, and read the new page id from the
// kind-tagged response (a session reports its page via `page_id`, the document
// kinds via `id`).
async function createByKind(
  campaignId: CampaignId,
  kind: PageKind,
  name: string | null,
  parent: PageId | null,
): Promise<PageId> {
  const { data, response } = await campaignClient.POST("/campaign/{id}/pages", {
    params: { path: { id: campaignId } },
    body: bodyFor(kind, name, parent),
  });
  if (!response.ok || data === undefined) {
    throw new Error(`Failed to create ${kind} (${response.status}).`);
  }
  // Re-brand the response id through the schema (validate-at-the-boundary). This
  // also sidesteps openapi-fetch widening PageId into a structural object form
  // the router's PageId param no longer accepts.
  return data.kind === "session"
    ? pageIdSchema.parse(data.content.page_id)
    : pageIdSchema.parse(data.content.id);
}

// Compose the kind-tagged request body. Exhaustive over PageKind: a new variant
// trips the `never` arm at compile time, mirroring the Rust `match`. Each kind
// carries only the fields it actually has.
function bodyFor(kind: PageKind, name: string | null, parent: PageId | null): CreatePageRequest {
  switch (kind) {
    case "entity":
      // Entity names are required; the modal gates an empty submit, so `name` is
      // a real string here. Cloning from a template is unbuilt (from_template_id
      // null; a value yields 501).
      return {
        kind: "entity",
        content: { name: name ?? "", status: null, parent, from_template_id: null },
      };
    case "template":
      // A template is named too (the modal gates it) and never clones from
      // another template, so it has no from_template_id.
      return { kind: "template", content: { name: name ?? "", status: null, parent } };
    case "session":
      // A session is named like every other kind now (the modal gates a blank
      // submit), and its name is unique among sessions (a duplicate yields 409).
      return { kind: "session", content: { name: name ?? "", status: null, parent } };
    default: {
      const _exhaustive: never = kind;
      throw new Error(`Unhandled PageKind: ${String(_exhaustive)}`);
    }
  }
}
