// Connector for the relationships widget: holds the fetch (useRelationships) and
// the create flow (useCreateRelationship), and hands the presentational widget +
// modal plain data and callbacks. HomeEditor mounts this only for entity/template
// pages, so the fetch lifecycle is tied to that conditional mount (a hook can't be
// called conditionally inside HomeEditor itself). Splitting the fetch + network
// glue from the presentational pieces keeps every state story-able with no socket,
// mirroring TocSidebar (connector) over TocTree/TocRow (presentational).

import type { CampaignId } from "@familiar-systems/types-app";
import type { PageId } from "@familiar-systems/types-campaign";
import { useState } from "react";

import { CreateRelationshipModal } from "./CreateRelationshipModal";
import { RelationshipsWidget } from "./RelationshipsWidget";
import { useCreateRelationship } from "./useCreateRelationship";
import { useRelationships } from "./useRelationships";

interface RelationshipsSectionProps {
  campaignId: CampaignId;
  pageId: PageId;
  pageKind: "entity" | "template";
  /** The current entity's name, shown as the fixed subject in the create flow. */
  subjectName: string;
}

export function RelationshipsSection({
  campaignId,
  pageId,
  pageKind,
  subjectName,
}: RelationshipsSectionProps): React.ReactElement {
  const isEntity = pageKind === "entity";
  // Templates show a static affordance, so they don't fetch and can't create
  // (the widget hides "+ add" for them, so `creating` stays false).
  const { state, refetch } = useRelationships(campaignId, pageId, isEntity);
  const [creating, setCreating] = useState(false);
  const create = useCreateRelationship(campaignId, creating);

  return (
    <>
      <RelationshipsWidget
        state={state}
        pageKind={pageKind}
        campaignId={campaignId}
        onAdd={() => setCreating(true)}
      />
      {/* Open only once the session list has loaded: the as-of picker defaults to
          the current session, so it needs the data before first paint. */}
      {creating && create.sessions !== null ? (
        <CreateRelationshipModal
          subjectName={subjectName}
          subjectPageId={pageId}
          predicates={create.predicates}
          sessions={create.sessions}
          onSearchEntities={create.searchEntities}
          onCreateEntity={create.createEntity}
          onSubmit={async (req) => {
            await create.submit(req);
            setCreating(false);
            refetch();
          }}
          onClose={() => setCreating(false)}
        />
      ) : null}
    </>
  );
}
