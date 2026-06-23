// Connector for the relationships widget: holds the fetch (useRelationships) and
// the create flow (useCreateRelationship), and hands the presentational widget +
// modal plain data and callbacks. HomeEditor mounts this only for entity/template
// pages, so the fetch lifecycle is tied to that conditional mount (a hook can't be
// called conditionally inside HomeEditor itself). Splitting the fetch + network
// glue from the presentational pieces keeps every state story-able with no socket,
// mirroring TocSidebar (connector) over TocTree/TocRow (presentational).

import type { CampaignId } from "@familiar-systems/types-app";
import type { PageId, RelationshipView } from "@familiar-systems/types-campaign";
import { useState } from "react";

import { CreateRelationshipModal } from "./CreateRelationshipModal";
import { EditRelationshipModal } from "./EditRelationshipModal";
import { RelationshipsWidget } from "./RelationshipsWidget";
import { useCreateRelationship } from "./useCreateRelationship";
import { useEditRelationship } from "./useEditRelationship";
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
  // The row being edited, or null when the edit modal is closed. Holding the whole
  // view (not just its id) lets the modal render the current line + supersede edges
  // without a refetch.
  const [editing, setEditing] = useState<RelationshipView | null>(null);
  const edit = useEditRelationship(campaignId, editing !== null);

  // Bind the edited row into a non-null local so the async onSubmit closure keeps
  // it (closures don't carry the `!== null` narrowing of the field on their own).
  // The modal opens only once the session list has loaded: the as-of picker
  // defaults to the current session, so it needs the data before first paint.
  let editModal: React.ReactNode = null;
  if (editing !== null && edit.sessions !== null) {
    const target = editing;
    editModal = (
      <EditRelationshipModal
        subjectName={subjectName}
        subjectPageId={pageId}
        view={target}
        sessions={edit.sessions}
        onSubmit={async (submit) => {
          await edit.apply(target.id, submit);
          setEditing(null);
          refetch();
        }}
        onClose={() => setEditing(null)}
      />
    );
  }

  return (
    <>
      <RelationshipsWidget
        state={state}
        pageKind={pageKind}
        campaignId={campaignId}
        onAdd={() => setCreating(true)}
        onSelect={setEditing}
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
      {editModal}
    </>
  );
}
