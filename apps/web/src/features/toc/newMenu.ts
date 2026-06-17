// The set of things the ToC "New" menu can create, plus each row's UI metadata.
//
// Keyed by the generated `PageKind` so the menu can't silently drift from the
// Rust enum: `satisfies Record<PageKind, NewMenuEntry | null>` turns a new
// variant (the planned `Skill` / `Memory`) into a compile error here until it is
// classified - the same exhaustiveness guarantee `PageKind`'s `match` arms hold
// on the Rust side (crates/campaign-shared/src/page_kind.rs). ts-rs emits a
// *type only* (no runtime value), so this hand-written mapping is unavoidable;
// keying it off the union is what keeps it honest. `null` = a real kind that is
// not creatable from this menu (reserved for a future kind that should not be
// hand-authored); all current kinds are creatable.

import type { PageKind } from "@familiar-systems/types-campaign";
import type { LucideIcon } from "lucide-react";

import { PAGE_KIND_ICON } from "./pageKindIcon";

/**
 * Which accent a row paints with. The session row is the gold "main event";
 * everything else is the neutral primary accent. Kept as a data field (not a
 * per-row ternary in the modal) so a new kind picks its accent here, and the
 * modal maps the token to concrete classes in one place.
 */
export type NewMenuColor = "gold" | "primary";

export interface NewMenuEntry {
  /** Row title (from the New menu design, verbatim). */
  label: string;
  /** Row subtitle (from the design). */
  subtitle: string;
  icon: LucideIcon;
  /** Pre-filled into the name field when this kind is chosen. */
  defaultName: string;
  /**
   * Whether a non-empty name is required. An entity must be named (`POST
   * /pages` rejects a blank name); a session may be blank (the server fills in
   * "Untitled Session").
   */
  nameRequired: boolean;
  /** Accent the picker row paints with. */
  color: NewMenuColor;
}

export const NEW_MENU = {
  session: {
    label: "New session",
    subtitle: "record audio, paste notes, upload a transcript",
    icon: PAGE_KIND_ICON.session,
    defaultName: "Untitled Session",
    nameRequired: false,
    color: "gold",
  },
  entity: {
    label: "New entity",
    subtitle: "a person, place, thing, or bit of lore",
    icon: PAGE_KIND_ICON.entity,
    defaultName: "",
    nameRequired: true,
    color: "primary",
  },
  template: {
    label: "New template",
    subtitle: "a reusable blueprint you clone new entities from",
    icon: PAGE_KIND_ICON.template,
    defaultName: "",
    nameRequired: true,
    color: "primary",
  },
} satisfies Record<PageKind, NewMenuEntry | null>;

/**
 * The creatable kinds with their metadata, in menu order. Object key order is
 * insertion order for string keys, which is the render order we want (Session
 * first). The cast narrows `Object.entries`' widened `string` keys back to
 * `PageKind`; sound because every key of `NEW_MENU` is a `PageKind`.
 */
export const NEW_MENU_ROWS: ReadonlyArray<{ kind: PageKind; entry: NewMenuEntry }> = (
  Object.entries(NEW_MENU) as [PageKind, NewMenuEntry | null][]
)
  .filter((pair): pair is [PageKind, NewMenuEntry] => pair[1] !== null)
  .map(([kind, entry]) => ({ kind, entry }));
