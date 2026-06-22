// Connector for the relationships widget: holds the fetch (useRelationships) and
// hands the widget plain `state`. HomeEditor mounts this only for entity/template
// pages, so the fetch lifecycle is tied to that conditional mount (a hook can't be
// called conditionally inside HomeEditor itself). Splitting the fetch from the
// presentational widget keeps every state story-able with no socket, mirroring
// TocSidebar (connector) over TocTree/TocRow (presentational).

import type { CampaignId } from "@familiar-systems/types-app";
import type { PageId } from "@familiar-systems/types-campaign";

import { RelationshipsWidget } from "./RelationshipsWidget";
import { useRelationships } from "./useRelationships";

interface RelationshipsSectionProps {
  campaignId: CampaignId;
  pageId: PageId;
  pageKind: "entity" | "template";
}

export function RelationshipsSection({
  campaignId,
  pageId,
  pageKind,
}: RelationshipsSectionProps): React.ReactElement {
  // Templates show a static affordance, so they don't fetch. The create/edit
  // flows (Slices 5/6) will pass onAdd/onSelect here and call the returned
  // `refetch` on success; read-only for now.
  const { state } = useRelationships(campaignId, pageId, pageKind === "entity");
  return <RelationshipsWidget state={state} pageKind={pageKind} campaignId={campaignId} />;
}
