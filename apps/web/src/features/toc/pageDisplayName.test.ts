import type { TocPageKind } from "@familiar-systems/types-campaign";
import { describe, expect, it } from "vitest";

import { pageDisplayName, pagePrefix } from "./pageDisplayName";

const entity: TocPageKind = { kind: "entity" };
const template: TocPageKind = { kind: "template" };
const session = (ordinal: number): TocPageKind => ({ kind: "session", ordinal });

describe("pagePrefix", () => {
  it("has no prefix for an entity", () => {
    expect(pagePrefix(entity)).toBeNull();
  });

  it("prefixes templates and sessions", () => {
    expect(pagePrefix(template)).toBe("Template:");
    expect(pagePrefix(session(3))).toBe("Session 3:");
  });
});

describe("pageDisplayName", () => {
  it("renders an entity as its name, falling back to Untitled", () => {
    expect(pageDisplayName(entity, "Korgath")).toBe("Korgath");
    expect(pageDisplayName(entity, "  ")).toBe("Untitled");
  });

  it("prefixes a template", () => {
    expect(pageDisplayName(template, "NPC Statblock")).toBe("Template: NPC Statblock");
  });

  it("composes a named session", () => {
    expect(pageDisplayName(session(3), "The Fall of Perth")).toBe("Session 3: The Fall of Perth");
  });

  // Names are required on every kind now, so a blank name shouldn't occur; these
  // guard the defensive fallback for stale CRDT data (render "Session 4", not the
  // dangling "Session 4:").
  it("defensively drops the trailing colon for a blank session name", () => {
    expect(pageDisplayName(session(4), "")).toBe("Session 4");
    expect(pageDisplayName(session(4), "   ")).toBe("Session 4");
  });

  it("defensively drops the trailing colon for a blank template name", () => {
    expect(pageDisplayName(template, "")).toBe("Template");
  });
});
