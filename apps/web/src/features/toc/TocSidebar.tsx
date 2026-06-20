// The campaign's table-of-contents sidebar: an IDE-style file explorer of the
// campaign's pages. Reads the live tree from the layout-pinned "toc" room
// (useToc; the campaign layout holds the acquire via useTocRoom), renders a
// drag-to-reorder/reparent tree, and creates new pages via the New menu modal
// (the new node arrives back over the same sync). Mounted at the /c/$campaignId
// layout so it persists across navigation between pages.

import type { CampaignId } from "@familiar-systems/types-app";
import type { PageId, PageKind } from "@familiar-systems/types-campaign";
import { useNavigate, useParams } from "@tanstack/react-router";
import type { TreeID } from "loro-crdt";
import { Plus } from "lucide-react";
import { useCallback, useState } from "react";

import { useLoroManager } from "../editor/LoroManagerProvider";
import { roomErrorMessage } from "../editor/loro-manager";
import { NewPageModal } from "./NewPageModal";
import { TocTree } from "./TocTree";
import { useToc } from "./useToc";
import { useCreatePage } from "./useCreatePage";

interface TocSidebarProps {
  campaignId: CampaignId;
}

export function TocSidebar({ campaignId }: TocSidebarProps): React.ReactElement {
  const manager = useLoroManager();
  const navigate = useNavigate();
  const createPage = useCreatePage(campaignId);
  const snapshot = useToc();

  // The open page comes from the child page route's param. We read it loosely
  // (the sidebar sits above that route); it is already URL-validated by the page
  // route's parseParams, so branding it here is safe.
  const params = useParams({ strict: false });
  const activePageId: PageId | null = (params.pageId ?? null) as PageId | null;

  // null = the New menu modal is closed. Otherwise it carries the parent the new
  // page nests under: `null` = ToC root, a PageId = under that page.
  const [newMenu, setNewMenu] = useState<{ parent: PageId | null } | null>(null);

  function goToPage(pageId: PageId): void {
    void navigate({ to: "/c/$campaignId/p/$pageId", params: { campaignId, pageId } });
  }

  function moveNode(node: TreeID, parent: TreeID | null, index: number): void {
    manager.moveTocNode(node, parent, index);
  }

  // Create through the owning route, then dismiss. Throwing on failure lets the
  // modal surface the error and stay open; success navigates (in createPage)
  // and clears the state, which unmounts the modal. Both handlers are stable
  // (`useCallback`) so the modal's document-level Escape listener binds once per
  // open, not on every sidebar re-render.
  const closeMenu = useCallback(() => setNewMenu(null), []);
  const handleCreate = useCallback(
    async (kind: PageKind, name: string | null): Promise<void> => {
      const parent = newMenu?.parent ?? null;
      await createPage(kind, name, parent);
      setNewMenu(null);
    },
    [newMenu, createPage],
  );

  return (
    <>
      <aside className="flex h-full w-64 shrink-0 flex-col border-r border-foreground/10 bg-background/50 backdrop-blur-sm">
        <div className="flex items-center justify-between gap-2 p-3">
          <span className="font-sans text-xs font-medium tracking-[0.18em] text-muted-foreground uppercase">
            Contents
          </span>
          <button
            type="button"
            aria-label="New page"
            onClick={() => setNewMenu({ parent: null })}
            className="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-primary/5 hover:text-primary"
          >
            <Plus className="size-4" />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
          <SidebarBody
            snapshot={snapshot}
            activePageId={activePageId}
            onNew={() => setNewMenu({ parent: null })}
            onNavigate={goToPage}
            onMove={moveNode}
            onAddChild={(parent) => setNewMenu({ parent })}
          />
        </div>
      </aside>

      {newMenu !== null ? <NewPageModal onSubmit={handleCreate} onClose={closeMenu} /> : null}
    </>
  );
}

interface SidebarBodyProps {
  snapshot: ReturnType<typeof useToc>;
  activePageId: PageId | null;
  onNew: () => void;
  onNavigate: (pageId: PageId) => void;
  onMove: (node: TreeID, parent: TreeID | null, index: number) => void;
  onAddChild: (parent: PageId) => void;
}

function SidebarBody({
  snapshot,
  activePageId,
  onNew,
  onNavigate,
  onMove,
  onAddChild,
}: SidebarBodyProps): React.ReactElement {
  switch (snapshot.status) {
    case "loading":
      return <p className="p-2 text-sm text-muted-foreground">Opening table of contents...</p>;
    case "error":
      return (
        <p className="p-2 text-sm text-red-700 dark:text-red-400">
          {roomErrorMessage(snapshot.error)}
        </p>
      );
    case "reconnecting":
    case "ready": {
      const reconnecting = snapshot.status === "reconnecting";
      // Only offer the empty-state CTA when genuinely ready; a transient
      // reconnect keeps the last-known tree (and its indicator) instead.
      if (snapshot.tree.length === 0 && !reconnecting) {
        return (
          <button
            type="button"
            onClick={onNew}
            className="mt-2 flex w-full items-center gap-1.5 rounded-md p-2 text-left text-sm text-muted-foreground transition-colors hover:bg-primary/5 hover:text-primary"
          >
            <Plus className="size-4" />
            <span>Create your first page</span>
          </button>
        );
      }
      return (
        <>
          {reconnecting ? (
            <p className="flex items-center gap-1.5 px-2 py-1 text-xs text-amber-500">
              <span className="size-1.5 animate-pulse rounded-full bg-amber-500" />
              Reconnecting...
            </p>
          ) : null}
          <TocTree
            tree={snapshot.tree}
            activePageId={activePageId}
            onNavigate={onNavigate}
            onMove={onMove}
            onAddChild={onAddChild}
          />
        </>
      );
    }
  }
}
