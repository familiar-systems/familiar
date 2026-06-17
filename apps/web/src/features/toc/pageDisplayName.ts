// Compose a page's display name from its kind and name. The ToC node and the
// page header render the same composition, so it lives in one place, switching
// on the `TocPageKind` sum: a future variant becomes a compile error here
// (exhaustive switch + `never`), and a session's ordinal is only in scope in the
// session arm (no nullable ordinal to guard).

import type { TocPageKind } from "@familiar-systems/types-campaign";

/**
 * The non-editable prefix for a page, or `null` when the kind has none (an
 * entity is just its name). Includes the trailing colon so a caller can place it
 * directly before the name (ToC) or before an editable name field (page header).
 */
export function pagePrefix(pageKind: TocPageKind): string | null {
  switch (pageKind.kind) {
    case "entity":
      return null;
    case "template":
      return "Template:";
    case "session":
      return `Session ${pageKind.ordinal}:`;
    default: {
      const _exhaustive: never = pageKind;
      throw new Error(`unhandled TocPageKind: ${JSON.stringify(_exhaustive)}`);
    }
  }
}

/**
 * The full display name for a non-editable context (the ToC sidebar). Built on
 * `pagePrefix`: a blank name drops the trailing colon ("Session 3", "Template"),
 * and an entity with no name falls back to "Untitled".
 */
export function pageDisplayName(pageKind: TocPageKind, name: string): string {
  const trimmed = name.trim();
  const prefix = pagePrefix(pageKind);
  if (prefix === null) return trimmed === "" ? "Untitled" : trimmed;
  // Drop the trailing colon when there is no name to follow it.
  return trimmed === "" ? prefix.replace(/:$/, "") : `${prefix} ${trimmed}`;
}
