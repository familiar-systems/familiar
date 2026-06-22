// Pure presentation logic for a relationship row, kept out of the component so it
// is unit-testable and so the Tailwind class tables live in one place (the same
// token->class discipline as pageKindIcon.ts / NewPageModal.tsx). Two orthogonal
// axes drive a row's look, mirroring the wireframe's composable `is-gm` /
// `is-superseded` / `is-retconned` classes:
//   - lifecycle (live | superseded | retconned), read off `invalidation`, owns the
//     predicate/chip text treatment;
//   - visibility (gm), read off `visibility`, owns the plum wash, applied as a
//     background layer so it composes over any lifecycle.

import type { RelationshipView, ViewSessionPoint } from "@familiar-systems/types-campaign";
import { EyeOff, History, X, type LucideIcon } from "lucide-react";

export type Lifecycle = "live" | "superseded" | "retconned";

/** Which lifecycle a row is in. `invalidation === null` is the live set. */
export function deriveLifecycle(view: RelationshipView): Lifecycle {
  if (view.invalidation === null) return "live";
  return view.invalidation.kind === "retconned" ? "retconned" : "superseded";
}

// "Session 3" / "Prior" for a live row's lone origin; "S3" / "Prior" for the
// compact ends of a range. The wireframe spells the live origin out and uses the
// terse S-number inside the superseded/retconned forms.
function longPoint(p: ViewSessionPoint): string {
  return p.kind === "prior" ? "Prior" : `Session ${p.content.ordinal}`;
}
function shortPoint(p: ViewSessionPoint): string {
  return p.kind === "prior" ? "Prior" : `S${p.content.ordinal}`;
}

/**
 * The origin chip text. Live: the origin alone ("Prior" / "Session 6").
 * Superseded: the span it held ("S6 → S12"). Retconned: its origin with the
 * retcon glyph ("S2 ↯").
 */
export function originLabel(view: RelationshipView): string {
  const inv = view.invalidation;
  if (inv === null) return longPoint(view.origin);
  if (inv.kind === "retconned") return `${shortPoint(view.origin)} ↯`;
  return `${shortPoint(view.origin)} → ${shortPoint(inv.content.ended)}`;
}

export type OriginTone = "normal" | "prior" | "ended" | "retcon";

/** The origin chip's color tone, following the same lifecycle/origin split. */
export function originTone(view: RelationshipView): OriginTone {
  const inv = view.invalidation;
  if (inv !== null) return inv.kind === "retconned" ? "retcon" : "ended";
  return view.origin.kind === "prior" ? "prior" : "normal";
}

/**
 * The right-gutter status glyph, by priority: a retcon/supersede mark wins over
 * the GM-only eye (a GM row that is also ended reads as ended; the wash still
 * signals GM-only). `null` for a plain live, player-visible row.
 */
export function gutterGlyph(
  view: RelationshipView,
): { Icon: LucideIcon; label: string; className: string } | null {
  const lifecycle = deriveLifecycle(view);
  if (lifecycle === "retconned") {
    return { Icon: X, label: "Retconned", className: "text-stone-400" };
  }
  if (lifecycle === "superseded") {
    return { Icon: History, label: "Superseded", className: "text-muted-foreground" };
  }
  if (view.visibility === "gm") {
    return { Icon: EyeOff, label: "GM only", className: "text-primary" };
  }
  return null;
}

// Predicate + chip treatment per lifecycle. Literal class strings so Tailwind's
// JIT can see them.
export const LIFECYCLE_STYLE = {
  live: {
    predicate: "text-foreground",
    chip: "bg-gold/10 text-foreground shadow-[inset_0_-1px_0] shadow-gold/35",
  },
  superseded: {
    predicate: "text-foreground/50 italic",
    chip: "bg-gold/[0.06] text-foreground/55 shadow-[inset_0_-1px_0] shadow-gold/20",
  },
  retconned: {
    predicate: "text-stone-400 line-through decoration-stone-400/50",
    chip: "bg-transparent text-stone-400 line-through decoration-stone-400/50",
  },
} satisfies Record<Lifecycle, { predicate: string; chip: string }>;

export const ORIGIN_TONE_CLASS = {
  normal: "border-foreground/15 text-muted-foreground",
  prior: "border-bronze/40 bg-bronze/5 text-bronze",
  ended: "border-foreground/20 bg-foreground/[0.03] text-muted-foreground",
  retcon: "border-stone-400/45 text-stone-400",
} satisfies Record<OriginTone, string>;
