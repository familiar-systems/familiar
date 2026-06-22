import type {
  RelationshipId,
  RelationshipView,
  ViewSessionPoint,
} from "@familiar-systems/types-campaign";
import { describe, expect, it } from "vitest";

import { deriveLifecycle, gutterGlyph, originLabel, originTone } from "./relationshipDisplay";

// The display logic only reads predicate/visibility/origin/invalidation, so the
// builder fills the rest with throwaway branded ids (cast like the TreeID
// fixtures in TocTree.stories.tsx - no runtime guard exists for these brands).
const session = (ordinal: number): ViewSessionPoint => ({ kind: "session", content: { ordinal } });
const prior: ViewSessionPoint = { kind: "prior" };

function view(overrides: Partial<RelationshipView>): RelationshipView {
  return {
    id: "01TEST00000000000000000REL" as RelationshipId,
    other: {
      id: "01OTHER0000000000000000PAG" as RelationshipView["other"]["id"],
      name: "Grimhollow",
    },
    predicate: "is a resident of",
    predicate_reverse: "is the home of",
    visibility: "players",
    origin: session(3),
    invalidation: null,
    ...overrides,
  };
}

describe("deriveLifecycle", () => {
  it("is live when not invalidated", () => {
    expect(deriveLifecycle(view({}))).toBe("live");
  });
  it("is superseded when ended", () => {
    expect(
      deriveLifecycle(
        view({ invalidation: { kind: "superseded", content: { ended: session(12) } } }),
      ),
    ).toBe("superseded");
  });
  it("is retconned when retconned", () => {
    expect(deriveLifecycle(view({ invalidation: { kind: "retconned" } }))).toBe("retconned");
  });
});

describe("originLabel", () => {
  it("spells out a live session origin", () => {
    expect(originLabel(view({ origin: session(3) }))).toBe("Session 3");
  });
  it("spells out a live prior origin", () => {
    expect(originLabel(view({ origin: prior }))).toBe("Prior");
  });
  it("renders a superseded span with compact session numbers", () => {
    expect(
      originLabel(
        view({
          origin: session(6),
          invalidation: { kind: "superseded", content: { ended: session(12) } },
        }),
      ),
    ).toBe("S6 → S12");
  });
  it("renders a prior-origin superseded span", () => {
    expect(
      originLabel(
        view({
          origin: prior,
          invalidation: { kind: "superseded", content: { ended: session(12) } },
        }),
      ),
    ).toBe("Prior → S12");
  });
  it("renders a retconned origin with the retcon glyph", () => {
    expect(originLabel(view({ origin: session(2), invalidation: { kind: "retconned" } }))).toBe(
      "S2 ↯",
    );
  });
});

describe("originTone", () => {
  it("is normal for a live session origin", () => {
    expect(originTone(view({ origin: session(3) }))).toBe("normal");
  });
  it("is prior for a live prior origin", () => {
    expect(originTone(view({ origin: prior }))).toBe("prior");
  });
  it("is ended for a superseded row", () => {
    expect(
      originTone(view({ invalidation: { kind: "superseded", content: { ended: session(12) } } })),
    ).toBe("ended");
  });
  it("is retcon for a retconned row", () => {
    expect(originTone(view({ invalidation: { kind: "retconned" } }))).toBe("retcon");
  });
});

describe("gutterGlyph priority", () => {
  it("is none for a live, player-visible row", () => {
    expect(gutterGlyph(view({}))).toBeNull();
  });
  it("is the eye for a live GM-only row", () => {
    expect(gutterGlyph(view({ visibility: "gm" }))?.label).toBe("GM only");
  });
  it("is the history mark for a superseded row", () => {
    expect(
      gutterGlyph(view({ invalidation: { kind: "superseded", content: { ended: session(12) } } }))
        ?.label,
    ).toBe("Superseded");
  });
  it("lets ended win over GM-only", () => {
    expect(
      gutterGlyph(
        view({
          visibility: "gm",
          invalidation: { kind: "superseded", content: { ended: session(12) } },
        }),
      )?.label,
    ).toBe("Superseded");
  });
  it("lets retcon win over GM-only", () => {
    expect(
      gutterGlyph(view({ visibility: "gm", invalidation: { kind: "retconned" } }))?.label,
    ).toBe("Retconned");
  });
});
