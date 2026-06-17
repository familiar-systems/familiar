// The default icon a page kind renders with - the single source of truth shared
// by the create menu (NEW_MENU) and the ToC, so a kind looks the same wherever it
// appears (one icon per concept). `satisfies Record<PageKind, ...>` makes a future
// kind (the planned Skill / Memory) a compile error here until it picks an icon,
// mirroring NEW_MENU's guard and the Rust `match` discipline in
// crates/campaign-shared/src/page_kind.rs.

import type { PageKind, TocPageKind } from "@familiar-systems/types-campaign";
import { LayoutTemplate, Mic, ScrollText, type LucideIcon } from "lucide-react";

export const PAGE_KIND_ICON = {
  entity: ScrollText, // authored world content / lore -> a scroll
  template: LayoutTemplate,
  session: Mic,
} satisfies Record<PageKind, LucideIcon>;

// ToC-only icon emphasis. The session is the campaign's spine, so it gets the gold
// "main event" accent (matching the New menu's session row); the other kinds stay
// quiet so session rows stand out in the dense nav list. Literal Tailwind classes
// so the JIT can see them - the same token->class discipline as NewPageModal.
const TOC_ICON_ACCENT: Record<PageKind, string> = {
  entity: "text-muted-foreground",
  template: "text-muted-foreground",
  session: "text-gold",
};

/**
 * The icon + accent class for a ToC page row. The one place a row's icon is
 * resolved: when per-page custom icons land, they layer in here as
 * `pageKind.icon ?? PAGE_KIND_ICON[kind]`, leaving TocRow and NEW_MENU untouched.
 */
export function tocRowIcon(pageKind: TocPageKind): { Icon: LucideIcon; className: string } {
  return {
    Icon: PAGE_KIND_ICON[pageKind.kind],
    className: TOC_ICON_ACCENT[pageKind.kind],
  };
}
