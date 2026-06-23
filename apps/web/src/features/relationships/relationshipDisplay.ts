// Pure presentation logic for a relationship row, kept out of the component so it
// is unit-testable and so the Tailwind class tables live in one place (the same
// token->class discipline as pageKindIcon.ts / NewPageModal.tsx). A relationship
// has two orthogonal axes; the row renders a temporal **rail** of pills plus a
// lifecycle treatment of the predicate/chip and a GM wash:
//   - the rail (`buildRail`) is the pills True-of -> Revealed-on -> Ended-on, in
//     session order, with a terminal Retcon pill always last;
//   - lifecycle (live | superseded | retconned), read off the factuality axis, owns
//     the predicate/chip text treatment;
//   - "currently secret" (born secret, not yet revealed), read off the knowledge
//     axis, owns the plum wash, applied as a background layer.

import type {
  KnowledgeView,
  RelationshipView,
  ViewSessionOrdinal,
  ViewSessionPoint,
} from "@familiar-systems/types-campaign";
import { Eye, EyeOff, RotateCcw, type LucideIcon } from "lucide-react";

export type Lifecycle = "live" | "superseded" | "retconned";

/** Which lifecycle a row is in. Factuality only: retcon wins over a plain end. */
export function deriveLifecycle(view: RelationshipView): Lifecycle {
  if (view.retcon !== null) return "retconned";
  if (view.superseded !== null) return "superseded";
  return "live";
}

/** A currently-secret, not-yet-revealed relationship: the GM-wash signal. */
export function isCurrentlySecret(view: RelationshipView): boolean {
  return view.knowledge.kind === "hidden";
}

export type PillTone = "origin" | "prior" | "secret" | "revealed" | "ended" | "retcon";

export type RailPill = {
  key: string;
  /** The session label, "Prior" or "S{n}". */
  label: string;
  /** A leading lucide icon (EyeOff = secret, Eye = revealed, RotateCcw = ended). */
  icon: LucideIcon | null;
  /** A leading text glyph - the retcon `↯`, which has no lucide equivalent. */
  glyph: string | null;
  tone: PillTone;
};

function sessionLabel(p: ViewSessionPoint): string {
  return p.kind === "prior" ? "Prior" : `S${p.content.ordinal}`;
}
function ordinalLabel(o: ViewSessionOrdinal): string {
  return `S${o.ordinal}`;
}
/** A point's sort value; `Prior` sorts before every session. */
function originSval(p: ViewSessionPoint): number {
  return p.kind === "prior" ? -1 : p.content.ordinal;
}

function reveal(k: KnowledgeView): ViewSessionOrdinal | null {
  return k.kind === "revealed" ? k.content : null;
}

/**
 * The row's temporal rail: pills in session order, retcon always last. Replaces the
 * single origin chip. Mirrors the wireframe's `renderRail`:
 *  - origin pill: a born-secret fact not revealed-coincident shows `secret` (EyeOff);
 *    else a plain `true` (no icon). At the origin point.
 *  - if ended and not retconned: an `ended` pill (RotateCcw) at the supersede session
 *    (a retcon absorbs the ended pill).
 *  - if born secret and revealed at a *different* session than origin: a `revealed`
 *    pill (Eye) at the reveal session.
 *  - if retconned: a terminal `↯ S{n}` pill, appended after the session sort.
 *
 * A reveal in the same session the fact became true reads as plain public (no hidden
 * interval): the origin pill is `true`, and there is no separate revealed pill.
 */
export function buildRail(view: RelationshipView): RailPill[] {
  const secret = view.knowledge.kind !== "public";
  const revealedAt = reveal(view.knowledge);
  const retconned = view.retcon !== null;
  const revealCoincides =
    revealedAt !== null &&
    view.origin.kind === "session" &&
    revealedAt.ordinal === view.origin.content.ordinal;
  const originSecret = secret && !revealCoincides;

  const pills: (RailPill & { sval: number })[] = [];

  pills.push({
    key: "origin",
    label: sessionLabel(view.origin),
    icon: originSecret ? EyeOff : null,
    glyph: null,
    tone: originSecret ? "secret" : view.origin.kind === "prior" ? "prior" : "origin",
    sval: originSval(view.origin),
  });

  if (view.superseded !== null && !retconned) {
    pills.push({
      key: "ended",
      label: ordinalLabel(view.superseded),
      icon: RotateCcw,
      glyph: null,
      tone: "ended",
      sval: view.superseded.ordinal,
    });
  }

  if (secret && revealedAt !== null && !revealCoincides) {
    pills.push({
      key: "revealed",
      label: ordinalLabel(revealedAt),
      icon: Eye,
      glyph: null,
      tone: "revealed",
      sval: revealedAt.ordinal,
    });
  }

  pills.sort((a, b) => a.sval - b.sval);
  const rail: RailPill[] = pills.map(({ sval: _sval, ...pill }) => pill);

  if (view.retcon !== null) {
    rail.push({
      key: "retcon",
      label: ordinalLabel(view.retcon),
      icon: null,
      glyph: "↯",
      tone: "retcon",
    });
  }

  return rail;
}

// Predicate + chip treatment per lifecycle. Literal class strings so Tailwind's
// JIT can see them.
export const LIFECYCLE_STYLE = {
  live: {
    predicate: "text-foreground",
    chip: "bg-bronze/10 text-foreground shadow-[inset_0_-1px_0] shadow-bronze/35",
  },
  superseded: {
    predicate: "text-foreground/50 italic",
    chip: "bg-bronze/[0.06] text-foreground/55 shadow-[inset_0_-1px_0] shadow-bronze/20",
  },
  retconned: {
    predicate: "text-stone-400 line-through decoration-stone-400/50",
    chip: "bg-transparent text-stone-400 line-through decoration-stone-400/50",
  },
} satisfies Record<Lifecycle, { predicate: string; chip: string }>;

export const RAIL_TONE_CLASS = {
  origin: "border-foreground/15 text-muted-foreground",
  prior: "border-bronze/40 bg-bronze/5 text-bronze",
  secret: "border-primary/40 bg-primary/5 text-primary",
  revealed: "border-foreground/20 bg-foreground/[0.03] text-foreground/70",
  ended: "border-foreground/20 bg-foreground/[0.03] text-muted-foreground",
  retcon: "border-stone-400/45 text-stone-400",
} satisfies Record<PillTone, string>;
