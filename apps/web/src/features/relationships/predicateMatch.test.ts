import type { PredicatePairView } from "@familiar-systems/types-campaign";
import { describe, expect, it } from "vitest";

import { filterPredicates, reverseFor } from "./predicateMatch";

const PAIRS: PredicatePairView[] = [
  { forward: "is a resident of", reverse: "is the home of", count: 42 },
  { forward: "is suspicious of", reverse: "is distrusted by", count: 18 },
  { forward: "is captain of", reverse: "is captained by", count: 9 },
  { forward: "keeps the key to", reverse: "is kept by", count: 3 },
];

describe("filterPredicates", () => {
  it("returns a copy of all pairs on an empty query", () => {
    const out = filterPredicates(PAIRS, "  ");
    expect(out).toHaveLength(PAIRS.length);
    expect(out).not.toBe(PAIRS);
  });
  it("matches the forward string case-insensitively", () => {
    expect(filterPredicates(PAIRS, "CAPTAIN").map((p) => p.forward)).toEqual(["is captain of"]);
  });
  it("ranks an earlier match above a later one", () => {
    // "is " starts every forward; "key" appears later in "keeps the key to".
    const out = filterPredicates(PAIRS, "is ");
    expect(out[0]?.forward.indexOf("is ")).toBe(0);
  });
  it("breaks ties on usage count", () => {
    // Both "is a resident of" and "is suspicious of" match "is " at index 0; the
    // higher-count one wins.
    const out = filterPredicates(PAIRS, "is ");
    expect(out[0]?.forward).toBe("is a resident of");
  });
  it("never lets usage count override match position", () => {
    // A popular deep match must not outrank an unpopular start-of-string match: count
    // is only a tiebreak between equal positions.
    const pairs: PredicatePairView[] = [
      { forward: "warden of", reverse: "warded by", count: 1 }, // "ward" at index 0
      { forward: "is steward of the ward", reverse: "stewarded by", count: 99 }, // index 6
    ];
    expect(filterPredicates(pairs, "ward").map((p) => p.forward)).toEqual([
      "warden of",
      "is steward of the ward",
    ]);
  });
  it("excludes non-matches", () => {
    expect(filterPredicates(PAIRS, "betrayed")).toEqual([]);
  });
});

describe("reverseFor", () => {
  it("finds the reverse when the query is a forward label", () => {
    expect(reverseFor(PAIRS, "is a resident of")).toBe("is the home of");
  });
  it("finds the forward when the query is a reverse label (canonicalization)", () => {
    // The pair may be stored with the GM's predicate in the reverse slot.
    expect(reverseFor(PAIRS, "is the home of")).toBe("is a resident of");
  });
  it("is case-insensitive and trims", () => {
    expect(reverseFor(PAIRS, "  Is Captain Of ")).toBe("is captained by");
  });
  it("returns null for an unknown predicate", () => {
    expect(reverseFor(PAIRS, "betrayed")).toBeNull();
  });
  it("returns null for an empty query", () => {
    expect(reverseFor(PAIRS, "")).toBeNull();
  });
});
