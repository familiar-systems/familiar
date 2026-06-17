import {
  pageIdSchema,
  TOC_KEY_KIND,
  TOC_KEY_PAGE_ID,
  TOC_KEY_PAGE_KIND,
  TOC_KEY_TITLE,
  TOC_KEY_VISIBILITY,
  TOC_KIND_FOLDER,
  TOC_KIND_PAGE,
  type PageId,
  type TocEntry,
} from "@familiar-systems/types-campaign";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { LoroDoc, type TreeID } from "loro-crdt";
import { expect, fn } from "storybook/test";

import { getTocTree, readTocTree, type TocTreeNode } from "./toc-doc";
import { TocTree } from "./TocTree";

// Fixture builders mirror tree-utils.test.ts. Because TocTree takes a plain
// `tree` array + callbacks (no Loro doc, no socket), the whole sidebar renders
// from in-memory data: the doc is the contract, transport is not involved.
const tid = (n: number): TreeID => `${n}@0` as TreeID;
const pid = (s: string): PageId => pageIdSchema.parse(s);

function folder(title: string): TocEntry {
  return { kind: "folder", title, visibility: "known", suggestions: [] };
}
function page(title: string, pageId: PageId): TocEntry {
  return {
    kind: "page",
    title,
    pageId,
    pageKind: { kind: "entity" },
    visibility: "known",
    suggestions: [],
  };
}
function templatePage(title: string, pageId: PageId): TocEntry {
  return {
    kind: "page",
    title,
    pageId,
    pageKind: { kind: "template" },
    visibility: "known",
    suggestions: [],
  };
}
function sessionPage(title: string, pageId: PageId, ordinal: number): TocEntry {
  return {
    kind: "page",
    title,
    pageId,
    pageKind: { kind: "session", ordinal },
    visibility: "known",
    suggestions: [],
  };
}
function node(id: TreeID, entry: TocEntry, children: TocTreeNode[] = []): TocTreeNode {
  return { treeId: id, entry, children };
}

const HOLLOW_KING = pid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const GREYMOOR = pid("01BX5ZZKBKACTAV9WEVGEMMVRY");
const ASHEN_PACT = pid("01CSGZ8M4Q9N7P2K3J5H6T8WXR");
const LOOSE_NOTES = pid("01D78XYFJ1E2K3M4N5P6Q7R8S9");

// Lore/{King, Greymoor}, Factions/{Ashen Pact}, then a root-level page.
const tree: TocTreeNode[] = [
  node(tid(1), folder("Lore"), [
    node(tid(2), page("The Hollow King", HOLLOW_KING)),
    node(tid(3), page("Castle Greymoor", GREYMOOR)),
  ]),
  node(tid(4), folder("Factions"), [node(tid(5), page("The Ashen Pact", ASHEN_PACT))]),
  node(tid(6), page("Loose Notes", LOOSE_NOTES)),
];

// The same shape, but built from a real in-process LoroDoc and read back via
// readTocTree, exactly as the app derives the tree from the synced "toc" room.
// This exercises the loro-crdt wasm pipeline inside the browser-mode test tier
// and is the in-process-LoroDoc fixture pattern the strategy doc prescribes for
// Loro-backed work: the doc is the contract, no socket involved.
function addFolder(doc: LoroDoc, parent: TreeID | undefined, title: string): TreeID {
  const created = getTocTree(doc).createNode(parent);
  created.data.set(TOC_KEY_KIND, TOC_KIND_FOLDER);
  created.data.set(TOC_KEY_TITLE, title);
  created.data.set(TOC_KEY_VISIBILITY, "known");
  return created.id;
}
function addPage(doc: LoroDoc, parent: TreeID | undefined, title: string, pageId: PageId): TreeID {
  const created = getTocTree(doc).createNode(parent);
  created.data.set(TOC_KEY_KIND, TOC_KIND_PAGE);
  created.data.set(TOC_KEY_TITLE, title);
  created.data.set(TOC_KEY_PAGE_ID, pageId);
  created.data.set(TOC_KEY_PAGE_KIND, "entity");
  created.data.set(TOC_KEY_VISIBILITY, "known");
  return created.id;
}

const loroBackedTree: TocTreeNode[] = (() => {
  const doc = new LoroDoc();
  const lore = addFolder(doc, undefined, "Lore");
  addPage(doc, lore, "The Hollow King", HOLLOW_KING);
  addPage(doc, lore, "Castle Greymoor", GREYMOOR);
  const factions = addFolder(doc, undefined, "Factions");
  addPage(doc, factions, "The Ashen Pact", ASHEN_PACT);
  addPage(doc, undefined, "Loose Notes", LOOSE_NOTES);
  doc.commit();
  return readTocTree(doc);
})();

const meta = {
  title: "Features/Toc/TocTree",
  component: TocTree,
  // The ToC is a left sidebar; render it at a realistic width, left-aligned.
  decorators: [
    (Story) => (
      <div style={{ width: 280, textAlign: "left" }}>
        <Story />
      </div>
    ),
  ],
  args: {
    tree,
    activePageId: null,
    onNavigate: fn(),
    onMove: fn(),
    onAddChild: fn(),
  },
} satisfies Meta<typeof TocTree>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Lore")).toBeInTheDocument();
    await expect(canvas.getByText("The Hollow King")).toBeInTheDocument();
    await expect(canvas.getByText("The Ashen Pact")).toBeInTheDocument();
  },
};

// The active page is highlighted; a visual-only state for the workshop.
export const WithActivePage: Story = {
  args: { activePageId: HOLLOW_KING },
};

// Templates and sessions compose their kind/ordinal into the row label; an
// unnamed session shows just "Session {ordinal}".
export const KindPrefixes: Story = {
  args: {
    tree: [
      node(tid(1), templatePage("NPC Statblock", HOLLOW_KING)),
      node(tid(2), sessionPage("The Fall of Perth", GREYMOOR, 3)),
      node(tid(3), sessionPage("", ASHEN_PACT, 4)),
    ],
  },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Template: NPC Statblock")).toBeInTheDocument();
    await expect(canvas.getByText("Session 3: The Fall of Perth")).toBeInTheDocument();
    await expect(canvas.getByText("Session 4")).toBeInTheDocument();
  },
};

// Identical content to Default, but the tree is derived from a real LoroDoc.
// Proves loro-crdt's wasm loads and runs in the browser-mode test tier.
export const FromLoroDoc: Story = {
  args: { tree: loroBackedTree },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Lore")).toBeInTheDocument();
    await expect(canvas.getByText("The Hollow King")).toBeInTheDocument();
  },
};

// Clicking a page row fires onNavigate with that page's id — the prop wiring
// that no pure test can reach.
export const Navigates: Story = {
  play: async ({ args, canvas, userEvent }) => {
    await userEvent.click(canvas.getByRole("button", { name: "The Hollow King" }));
    await expect(args.onNavigate).toHaveBeenCalledWith(HOLLOW_KING);
  },
};

// Clicking a page row's "Add sub-page" fires onAddChild with that page's id.
// This is the seam the New-menu modal hangs off: TocSidebar turns the call into
// `setNewMenu({ parent })`, which becomes `createPage(kind, name, parent)`. The
// add-sub-page button renders for pages only, so flatten order
// [Lore, King, Greymoor, Factions, Ashen, Notes] makes index 0 King's.
export const AddsChild: Story = {
  play: async ({ args, canvas, userEvent }) => {
    const adders = canvas.getAllByRole("button", { name: "Add sub-page" });
    const kingAdder = adders[0];
    if (kingAdder === undefined) throw new Error("expected King's add-sub-page button");
    await userEvent.click(kingAdder);
    await expect(args.onAddChild).toHaveBeenCalledWith(HOLLOW_KING);
  },
};

// A real-browser keyboard drag through dnd-kit's KeyboardSensor: pick up "The
// Hollow King" (its grip handle), move it down one slot, drop. Asserts the full
// gesture -> getProjection -> getMovePlacement -> onMove wiring fires in a real
// browser. tree-utils.test.ts covers only the placement math, never the dnd-kit
// event flow, and jsdom can't run this (dnd-kit reads getBoundingClientRect).
export const ReordersByKeyboard: Story = {
  play: async ({ args, canvas, userEvent }) => {
    // Grip handles share an aria-label; flatten order is
    // [Lore, King, Greymoor, Factions, Ashen, Notes], so index 1 is King's.
    const grips = canvas.getAllByRole("button", { name: "Drag to reorder" });
    const kingGrip = grips[1];
    if (kingGrip === undefined) throw new Error("expected King's drag handle");

    kingGrip.focus();
    await userEvent.keyboard("[Space]"); // pick up
    await userEvent.keyboard("[ArrowDown]"); // move down one slot
    await userEvent.keyboard("[Space]"); // drop

    await expect(args.onMove).toHaveBeenCalled();
    // The first onMove argument is always the dragged node's TreeID.
    expect(args.onMove.mock.calls[0]?.[0]).toBe(tid(2));
  },
};
