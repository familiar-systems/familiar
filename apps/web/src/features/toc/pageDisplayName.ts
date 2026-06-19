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
 * `pagePrefix`: a prefixed kind renders "Session 3: Name" / "Template: Name"; an
 * unnamed entity (no prefix) falls back to "Untitled".
 *
 * Every kind requires a name now, so a blank name shouldn't reach here. The
 * blank-name branches stay as a defensive fallback for stale CRDT data, so we'd
 * still render "Session 3" rather than a dangling "Session 3:".
 */
export function pageDisplayName(pageKind: TocPageKind, name: string): string {
  const trimmed = name.trim();
  const prefix = pagePrefix(pageKind);
  if (prefix === null) return trimmed === "" ? "Untitled" : trimmed;
  // Defensive (see above): drop the trailing colon when there is no name.
  return trimmed === "" ? prefix.replace(/:$/, "") : `${prefix} ${trimmed}`;
}
