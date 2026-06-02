// The campaign's table-of-contents sidebar: an IDE-style file explorer of the
// campaign's pages. Reads the live tree from the always-on "toc" room (useToc),
// renders a drag-to-reorder/reparent tree, and creates new pages via REST (the
// new node arrives back over the same sync). Mounted at the /c/$campaignId layout
// so it persists across navigation between pages.

import type { CampaignId } from "@familiar-systems/types-app";
import type { ThingId } from "@familiar-systems/types-campaign";
import { useNavigate, useParams } from "@tanstack/react-router";
import type { TreeID } from "loro-crdt";
import { Plus } from "lucide-react";
import { useState } from "react";

import { useLoroManager } from "../editor/LoroManagerProvider";
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

  // The open page comes from the child thing route's param. We read it loosely
  // (the sidebar sits above that route); it is already URL-validated by the thing
  // route's parseParams, so branding it here is safe.
  const params = useParams({ strict: false });
  const activeThingId: ThingId | null = (params.thingId ?? null) as ThingId | null;

  // undefined = not creating, null = creating at root, ThingId = under that page.
  const [pendingParent, setPendingParent] = useState<ThingId | null | undefined>(undefined);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  function goToPage(thingId: ThingId): void {
    void navigate({ to: "/c/$campaignId/t/$thingId", params: { campaignId, thingId } });
  }

  function moveNode(node: TreeID, parent: TreeID | null, index: number): void {
    manager.moveTocNode(node, parent, index);
  }

  function cancelCreate(): void {
    if (!creating) {
      setPendingParent(undefined);
      setCreateError(null);
    }
  }

  async function submitCreate(name: string): Promise<void> {
    setCreating(true);
    setCreateError(null);
    try {
      await createPage(name, pendingParent ?? null);
      setPendingParent(undefined);
    } catch (err) {
      setCreateError(err instanceof Error ? err.message : "Failed to create page.");
    } finally {
      setCreating(false);
    }
  }

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-foreground/10 bg-background/50 backdrop-blur-sm">
      <div className="flex items-center justify-between gap-2 px-3 py-3">
        <span className="font-sans text-xs font-medium tracking-[0.18em] text-muted-foreground uppercase">
          Contents
        </span>
        <button
          type="button"
          aria-label="New page"
          onClick={() => setPendingParent(null)}
          className="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-primary/5 hover:text-primary"
        >
          <Plus className="size-4" />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4">
        <SidebarBody
          snapshot={snapshot}
          activeThingId={activeThingId}
          pendingParent={pendingParent}
          creating={creating}
          onNew={() => setPendingParent(null)}
          onNavigate={goToPage}
          onMove={moveNode}
          onAddChild={(parent) => setPendingParent(parent)}
          onSubmitCreate={(name) => void submitCreate(name)}
          onCancelCreate={cancelCreate}
        />
      </div>

      {createError !== null ? (
        <p className="px-3 pb-3 text-xs text-red-700 dark:text-red-400">{createError}</p>
      ) : null}
    </aside>
  );
}

interface SidebarBodyProps {
  snapshot: ReturnType<typeof useToc>;
  activeThingId: ThingId | null;
  pendingParent: ThingId | null | undefined;
  creating: boolean;
  onNew: () => void;
  onNavigate: (thingId: ThingId) => void;
  onMove: (node: TreeID, parent: TreeID | null, index: number) => void;
  onAddChild: (parent: ThingId) => void;
  onSubmitCreate: (name: string) => void;
  onCancelCreate: () => void;
}

function SidebarBody({
  snapshot,
  activeThingId,
  pendingParent,
  creating,
  onNew,
  onNavigate,
  onMove,
  onAddChild,
  onSubmitCreate,
  onCancelCreate,
}: SidebarBodyProps): React.ReactElement {
  switch (snapshot.status) {
    case "loading":
      return (
        <p className="px-2 py-2 text-sm text-muted-foreground">Opening table of contents...</p>
      );
    case "error":
      return <p className="px-2 py-2 text-sm text-red-700 dark:text-red-400">{snapshot.message}</p>;
    case "ready":
      if (snapshot.tree.length === 0 && pendingParent === undefined) {
        return (
          <button
            type="button"
            onClick={onNew}
            className="mt-2 flex w-full items-center gap-1.5 rounded-md px-2 py-2 text-left text-sm text-muted-foreground transition-colors hover:bg-primary/5 hover:text-primary"
          >
            <Plus className="size-4" />
            <span>Create your first page</span>
          </button>
        );
      }
      return (
        <TocTree
          tree={snapshot.tree}
          activeThingId={activeThingId}
          pendingParent={pendingParent}
          creating={creating}
          onNavigate={onNavigate}
          onMove={onMove}
          onAddChild={onAddChild}
          onSubmitCreate={onSubmitCreate}
          onCancelCreate={onCancelCreate}
        />
      );
  }
}
