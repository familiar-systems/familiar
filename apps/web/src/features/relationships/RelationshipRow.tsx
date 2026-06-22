// One relationship as a row: the forward predicate, the linked "other" entity
// chip, an origin chip, and a right-gutter status glyph. All five visual states
// (live, prior, superseded, GM-washed, retconned) come from the pure tables in
// relationshipDisplay. Presentational: `onSelect` opens the edit flow (wired in a
// later slice); read-only callers omit it and the row is static. The chip always
// links to the other page and stops propagation, so chip-nav never doubles as
// row-select.

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

  // TODO(Slice 6): when onSelect is wired, two things activate that are inert now.
  // (1) The GM gradient branch below short-circuits the `interactive ? hover`
  // branch, so an interactive GM row gets no hover feedback; the wireframe layers
  // hover *over* the wash (a separate ::before), so both should coexist. (2) An
  // interactive row becomes role="button" wrapping the chip <Link> (<a>), nesting
  // interactive controls - resolve both when the row becomes clickable.
  const select = onSelect === undefined ? undefined : () => onSelect(view);
  const onKeyDown =
    onSelect === undefined
      ? undefined
      : (e: React.KeyboardEvent) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelect(view);
          }
        };
  const interactive = select !== undefined;

  // The predicate verb is muted by default; a live GM fact tints it plum to match
  // the wash. Superseded/retconned rows let the verb inherit the row's faded or
  // struck-through color rather than fighting it.
  const verbClass =
    lifecycle === "live" ? (isGm ? "text-primary/85" : "text-muted-foreground") : "";

  return (
    <div
      data-predicate-forward={view.predicate}
      data-predicate-reverse={view.predicate_reverse}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onClick={select}
      onKeyDown={onKeyDown}
      className={[
        "relative grid grid-cols-[1fr_auto] items-baseline gap-4 rounded py-2 pr-11 pl-2.5 transition-colors",
        isGm
          ? "bg-gradient-to-r from-primary/[0.12] via-primary/[0.06] to-transparent"
          : interactive
            ? "hover:bg-gold/[0.07]"
            : "",
        interactive ? "cursor-pointer" : "",
      ].join(" ")}
    >
      <div className={["font-sans text-[15px] leading-snug", style.predicate].join(" ")}>
        <em className={["mr-1.5 italic", verbClass].join(" ")}>{view.predicate}</em>
        <Link
          to="/c/$campaignId/p/$pageId"
          params={{ campaignId, pageId: view.other.id }}
          onClick={(e) => e.stopPropagation()}
          className={[
            "inline-flex items-baseline rounded px-1.5 font-display font-semibold",
            style.chip,
          ].join(" ")}
        >
          {view.other.name}
        </Link>
      </div>

      <span
        className={[
          "justify-self-end rounded-full border px-1.5 py-0.5 font-sans text-[10.5px] tracking-wide uppercase",
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
