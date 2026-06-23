import type {
  KnowledgeView,
  RelationshipId,
  RelationshipView,
  ViewSessionOrdinal,
  ViewSessionPoint,
} from "@familiar-systems/types-campaign";
import { describe, expect, it } from "vitest";

import { buildRail, deriveLifecycle, isCurrentlySecret } from "./relationshipDisplay";

// The display logic reads only predicate/origin/superseded/retcon/knowledge, so the
// builder fills the rest with throwaway branded ids (cast like the TreeID fixtures
// in TocTree.stories.tsx - no runtime guard exists for these brands).
const ord = (ordinal: number): ViewSessionOrdinal => ({ ordinal });
const session = (ordinal: number): ViewSessionPoint => ({ kind: "session", content: { ordinal } });
const prior: ViewSessionPoint = { kind: "prior" };
const revealed = (ordinal: number): KnowledgeView => ({ kind: "revealed", content: { ordinal } });

function view(overrides: Partial<RelationshipView>): RelationshipView {
  return {
    id: "01TEST00000000000000000REL" as RelationshipId,
    other: {
      id: "01OTHER0000000000000000PAG" as RelationshipView["other"]["id"],
      name: "Grimhollow",
    },
    predicate: "is a resident of",
    predicate_reverse: "is the home of",
    origin: session(3),
    superseded: null,
    retcon: null,
    knowledge: { kind: "public" },
    ...overrides,
  };
}

/** A compact rail shape for assertions: label, tone, and any leading glyph. */
function rail(v: RelationshipView) {
  return buildRail(v).map((p) => ({ label: p.label, tone: p.tone, glyph: p.glyph }));
}

describe("deriveLifecycle", () => {
  it("is live when neither superseded nor retconned", () => {
    expect(deriveLifecycle(view({}))).toBe("live");
  });
  it("is superseded when ended", () => {
    expect(deriveLifecycle(view({ superseded: ord(12) }))).toBe("superseded");
  });
  it("is retconned when retconned", () => {
    expect(deriveLifecycle(view({ retcon: ord(2) }))).toBe("retconned");
  });
  it("lets retcon win over a coexisting supersede", () => {
    expect(deriveLifecycle(view({ superseded: ord(12), retcon: ord(14) }))).toBe("retconned");
  });
});

describe("isCurrentlySecret", () => {
  it("is true only for a hidden (born-secret, unrevealed) row", () => {
    expect(isCurrentlySecret(view({ knowledge: { kind: "hidden" } }))).toBe(true);
    expect(isCurrentlySecret(view({ knowledge: { kind: "public" } }))).toBe(false);
    expect(isCurrentlySecret(view({ knowledge: revealed(5) }))).toBe(false);
  });
});

describe("buildRail", () => {
  it("a live prior public row: a lone Prior pill", () => {
    expect(rail(view({ origin: prior }))).toEqual([{ label: "Prior", tone: "prior", glyph: null }]);
  });

  it("a live session public row: a lone session pill", () => {
    expect(rail(view({ origin: session(3) }))).toEqual([
      { label: "S3", tone: "origin", glyph: null },
    ]);
  });

  it("a superseded row: origin then ended, in session order", () => {
    expect(rail(view({ origin: session(6), superseded: ord(12) }))).toEqual([
      { label: "S6", tone: "origin", glyph: null },
      { label: "S12", tone: "ended", glyph: null },
    ]);
  });

  it("a live born-secret row: a secret origin pill (GM-washed)", () => {
    const v = view({ origin: session(11), knowledge: { kind: "hidden" } });
    expect(rail(v)).toEqual([{ label: "S11", tone: "secret", glyph: null }]);
    expect(isCurrentlySecret(v)).toBe(true);
  });

  it("a born-secret then revealed row: secret origin then revealed", () => {
    expect(rail(view({ origin: session(14), knowledge: revealed(15) }))).toEqual([
      { label: "S14", tone: "secret", glyph: null },
      { label: "S15", tone: "revealed", glyph: null },
    ]);
  });

  it("a born-secret superseded row: both axes show (secret origin + ended)", () => {
    expect(
      rail(view({ origin: session(8), superseded: ord(11), knowledge: { kind: "hidden" } })),
    ).toEqual([
      { label: "S8", tone: "secret", glyph: null },
      { label: "S11", tone: "ended", glyph: null },
    ]);
  });

  it("a retconned row: origin then a terminal retcon glyph", () => {
    expect(rail(view({ origin: session(1), retcon: ord(2) }))).toEqual([
      { label: "S1", tone: "origin", glyph: null },
      { label: "S2", tone: "retcon", glyph: "↯" },
    ]);
  });

  it("retcon absorbs the ended pill and is always last", () => {
    // Ended S12 then retconned S14: no ended pill, retcon terminal.
    expect(rail(view({ origin: session(6), superseded: ord(12), retcon: ord(14) }))).toEqual([
      { label: "S6", tone: "origin", glyph: null },
      { label: "S14", tone: "retcon", glyph: "↯" },
    ]);
  });

  it("a reveal coincident with origin reads as plain public (no secret, no reveal pill)", () => {
    const v = view({ origin: session(5), knowledge: revealed(5) });
    expect(rail(v)).toEqual([{ label: "S5", tone: "origin", glyph: null }]);
    expect(isCurrentlySecret(v)).toBe(false);
  });
});
