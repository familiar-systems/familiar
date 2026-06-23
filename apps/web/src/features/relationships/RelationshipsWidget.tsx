// The relationships widget rendered at an entity/template page's preamble/body
// seam: a header (title, count, "+ add") over the relationship rows, with loading
// / error / empty branches. Presentational and state-driven (mirrors TocSidebar's
// SidebarBody switch): the connector RelationshipsSection feeds it `state`, so
// every branch renders from plain data in stories with no socket. A template
// shows an affordance instead of a list - relationships are authored on the
// entities cloned from a template, not on the template itself.

import type { CampaignId } from "@familiar-systems/types-app";
import type { RelationshipView } from "@familiar-systems/types-campaign";

import { RelationshipRow } from "./RelationshipRow";
import type { RelationshipsState } from "./useRelationships";

interface RelationshipsWidgetProps {
  state: RelationshipsState;
  pageKind: "entity" | "template";
  campaignId: CampaignId;
  /** Opens the create flow. Omitted = no "+ add". */
  onAdd?: () => void;
  /** Opens the edit flow for a row. Omitted = rows are static. */
  onSelect?: (view: RelationshipView) => void;
}

export function RelationshipsWidget({
  state,
  pageKind,
  campaignId,
  onAdd,
  onSelect,
}: RelationshipsWidgetProps): React.ReactElement {
  const isTemplate = pageKind === "template";
  const count = state.status === "ready" ? state.relationships.length : null;

  return (
    <section className="mt-10" aria-label="Relationships">
      <Header count={isTemplate ? null : count} onAdd={isTemplate ? undefined : onAdd} />
      {isTemplate ? (
        <p className="py-3 font-sans text-sm text-muted-foreground italic">
          Relationships appear here on entities created from this template.
        </p>
      ) : (
        <Body state={state} campaignId={campaignId} onSelect={onSelect} />
      )}
    </section>
  );
}

function Header({
  count,
  onAdd,
}: {
  count: number | null;
  onAdd?: (() => void) | undefined;
}): React.ReactElement {
  return (
    <div className="mb-1 flex items-baseline gap-2.5 border-b border-foreground/10 pb-1.5">
      <h2 className="font-display text-[15px] font-semibold tracking-wide text-muted-foreground uppercase">
        Relationships
      </h2>
      {count !== null ? (
        <span className="rounded-full border border-foreground/15 px-1.5 font-sans text-[11px] text-muted-foreground">
          {count}
        </span>
      ) : null}
      {onAdd !== undefined ? (
        <button
          type="button"
          onClick={onAdd}
          className="ml-auto font-sans text-[11px] text-primary hover:underline"
        >
          + add
        </button>
      ) : null}
    </div>
  );
}

function Body({
  state,
  campaignId,
  onSelect,
}: {
  state: RelationshipsState;
  campaignId: CampaignId;
  onSelect?: ((view: RelationshipView) => void) | undefined;
}): React.ReactElement {
  switch (state.status) {
    case "loading":
      return (
        <p className="py-3 font-sans text-sm text-muted-foreground">Loading relationships...</p>
      );
    case "error":
      return (
        <p className="py-3 font-sans text-sm text-red-700 dark:text-red-400">{state.message}</p>
      );
    case "ready":
      if (state.relationships.length === 0) {
        return (
          <p className="py-3 font-sans text-sm text-muted-foreground italic">
            No relationships yet.
          </p>
        );
      }
      return (
        <div className="divide-y divide-dashed divide-foreground/10">
          {state.relationships.map((view) => (
            <RelationshipRow
              key={view.id}
              view={view}
              campaignId={campaignId}
              onSelect={onSelect}
            />
          ))}
        </div>
      );
  }
}
