// Fuzzy match for the wizard's system picker (scriptorium variant).
// Substring scoring on `name` only; the wireframe used a richer alias
// list, but the catalog YAML doesn't carry aliases yet (open question
// in the design doc). When aliases land, extend the field array here.

import type { SystemEntry } from "@familiar-systems/types-campaign";

interface Scored {
  entry: SystemEntry;
  score: number;
}

export function fuzzyMatchSystems(systems: readonly SystemEntry[], query: string): SystemEntry[] {
  const q = query.trim().toLowerCase();
  if (q === "") {
    return systems.slice();
  }
  const scored: Scored[] = [];
  for (const entry of systems) {
    const fields = [entry.name];
    let best = -1;
    let isCanonical = false;
    for (const field of fields) {
      const idx = field.toLowerCase().indexOf(q);
      if (idx >= 0 && (best < 0 || idx < best)) {
        best = idx;
        isCanonical = true;
      }
    }
    if (best >= 0) {
      let score = 1000 - best * 10;
      if (isCanonical) {
        score += 200;
      }
      scored.push({ entry, score });
    }
  }
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.entry);
}
