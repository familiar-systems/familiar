// Pure matching for the create modal's predicate typeahead, kept out of the
// component so it is unit-testable (mirrors features/onboarding/fuzzyMatch.ts).
// The predicate vocabulary is small and fetched once, so this filters client-side
// (unlike the entity object search, which is a server query).

import type { PredicatePairView } from "@familiar-systems/types-campaign";

interface Scored {
  pair: PredicatePairView;
  score: number;
}

/**
 * Rank known predicate pairs by an `forward`-field substring match (earlier match
 * = higher), most-used as the tiebreak. The typeahead lists the `forward` string;
 * an empty query returns a copy of all pairs (so the dropdown can show the vocab).
 */
export function filterPredicates(
  pairs: readonly PredicatePairView[],
  query: string,
): PredicatePairView[] {
  const q = query.trim().toLowerCase();
  if (q === "") {
    return pairs.slice();
  }
  const scored: Scored[] = [];
  for (const pair of pairs) {
    const idx = pair.forward.toLowerCase().indexOf(q);
    if (idx >= 0) {
      // Earlier match dominates; `count` (how often the pair is used) breaks ties
      // so the campaign's established wording floats up.
      scored.push({ pair, score: 1000 - idx * 10 + Math.min(pair.count, 99) });
    }
  }
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.pair);
}

/**
 * The reverse predicate the graph already knows for `forward`, or null if unknown
 * (a custom predicate the GM must pair by hand). Pairs are canonicalized
 * server-side, so a predicate may sit in either slot: check both directions.
 * Exact, case-insensitive (autofill should only fire on a real match, not a
 * substring of a longer phrase the GM is still typing).
 */
export function reverseFor(pairs: readonly PredicatePairView[], forward: string): string | null {
  const f = forward.trim().toLowerCase();
  if (f === "") return null;
  for (const pair of pairs) {
    if (pair.forward.toLowerCase() === f) return pair.reverse;
    if (pair.reverse.toLowerCase() === f) return pair.forward;
  }
  return null;
}
