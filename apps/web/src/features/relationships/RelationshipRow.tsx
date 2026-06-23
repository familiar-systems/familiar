// One relationship as a row: the forward predicate, the linked "other" entity
// chip, an origin chip, and a right-gutter status glyph. All five visual states
// (live, prior, superseded, GM-washed, retconned) come from the pure tables in
// relationshipDisplay. Presentational: `onSelect` opens the edit flow; read-only
// callers omit it and the row is static. The chip always links to the other page
// and stops propagation, so chip-nav never doubles as row-select.

import type { CampaignId } from "@familiar-systems/types-app";
import type { RelationshipView } from "@familiar-systems/types-campaign";
import { Link } from "@tanstack/react-router";

import {
  deriveLifecycle,
  gutterGlyph,
  LIFECYCLE_STYLE,
  originLabel,
  ORIGIN_TONE_CLASS,
  originTone,
} from "./relationshipDisplay";

interface RelationshipRowProps {
  view: RelationshipView;
  campaignId: CampaignId;
  /** Opens the edit flow for this row. Omitted = read-only (static row). */
  onSelect?: ((view: RelationshipView) => void) | undefined;
}

export function RelationshipRow({
  view,
  campaignId,
  onSelect,
}: RelationshipRowProps): React.ReactElement {
  const lifecycle = deriveLifecycle(view);
  const style = LIFECYCLE_STYLE[lifecycle];
  const isGm = view.visibility === "gm";
  const glyph = gutterGlyph(view);

  // The predicate verb is muted by default; a live GM fact tints it plum to match
  // the wash. Superseded/retconned rows let the verb inherit the row's faded or
  // struck-through color rather than fighting it.
  const verbClass =
    lifecycle === "live" ? (isGm ? "text-primary/85" : "text-muted-foreground") : "";

  return (
    <div
      data-predicate-forward={view.predicate}
      data-predicate-reverse={view.predicate_reverse}
      className={[
        "relative isolate grid grid-cols-[1fr_auto] items-baseline gap-4 rounded py-2 pr-11 pl-2.5",
        // The GM wash is a backmost layer (not the row's own background), so the
        // edit button's hover tint composes over it instead of replacing it. With
        // role=button gone (see below), the two no longer fight for one background.
        isGm
          ? "before:absolute before:inset-0 before:-z-10 before:rounded before:bg-gradient-to-r before:from-primary/[0.12] before:via-primary/[0.06] before:to-transparent before:content-['']"
          : "",
      ].join(" ")}
    >
      {/* The whole row is the edit target, but as a real <button> that is a sibling
          of the chip <Link>, not a role=button wrapping it (which nests interactive
          controls). It sits just behind the in-flow content so its hover tint reads
          under the text; the content is click-transparent except the chip, which
          navigates to the other page. Native button = keyboard focus + Enter/Space
          for free. */}
      {onSelect !== undefined ? (
        <button
          type="button"
          aria-label={`Edit relationship: ${view.predicate} ${view.other.name}`}
          onClick={() => onSelect(view)}
          className="absolute inset-0 -z-[1] cursor-pointer rounded transition-colors hover:bg-gold/[0.07] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-gold/60"
        />
      ) : null}

      <div
        className={["pointer-events-none font-sans text-[15px] leading-snug", style.predicate].join(
          " ",
        )}
      >
        <em className={["mr-1.5 italic", verbClass].join(" ")}>{view.predicate}</em>
        <Link
          to="/c/$campaignId/p/$pageId"
          params={{ campaignId, pageId: view.other.id }}
          onClick={(e) => e.stopPropagation()}
          className={[
            "pointer-events-auto relative z-10 inline-flex items-baseline rounded px-1.5 font-display font-semibold",
            style.chip,
          ].join(" ")}
        >
          {view.other.name}
        </Link>
      </div>

      <span
        className={[
          "pointer-events-none justify-self-end rounded-full border px-1.5 py-0.5 font-sans text-[10.5px] tracking-wide uppercase",
          ORIGIN_TONE_CLASS[originTone(view)],
        ].join(" ")}
      >
        {originLabel(view)}
      </span>

      {glyph !== null ? (
        <span
          className={[
            "pointer-events-none absolute top-1/2 right-2 -translate-y-1/2",
            glyph.className,
          ].join(" ")}
        >
          <glyph.Icon className="size-4" aria-label={glyph.label} />
        </span>
      ) : null}
    </div>
  );
}
