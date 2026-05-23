import { useState } from "react";
import { Link, useParams } from "@tanstack/react-router";
import type { CampaignId } from "@familiar-systems/types-app";
import type { ThingId } from "@familiar-systems/types-campaign";
import { useToc } from "./LoroManagerProvider";
import { campaignClient } from "../../lib/campaigns-api";
import type { TocTreeEntry } from "../../lib/loro-manager";

interface TocSidebarProps {
  campaignId: string;
}

export function TocSidebar({ campaignId }: TocSidebarProps): React.ReactElement {
  const toc = useToc();
  const [creating, setCreating] = useState(false);

  async function handleCreate() {
    if (creating) return;
    setCreating(true);
    try {
      await campaignClient.POST("/campaign/{id}/things", {
        params: { path: { id: campaignId } },
        body: { name: "Untitled" },
      });
    } catch (err) {
      console.error("Failed to create thing:", err);
    } finally {
      setCreating(false);
    }
  }

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-foreground/10 bg-muted/30">
      <div className="flex items-center justify-between px-4 py-3 border-b border-foreground/10">
        <span className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Pages
        </span>
        <button
          type="button"
          onClick={() => void handleCreate()}
          disabled={creating}
          className="flex h-6 w-6 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground disabled:opacity-50"
          aria-label="Create new page"
        >
          +
        </button>
      </div>

      <nav className="flex-1 overflow-y-auto px-2 py-2">
        {toc.status === "loading" && (
          <p className="px-2 py-1 text-xs text-muted-foreground">Connecting...</p>
        )}
        {toc.status === "ready" && toc.entries.length === 0 && (
          <p className="px-2 py-4 text-center text-xs text-muted-foreground">
            No pages yet. Click + to create one.
          </p>
        )}
        {toc.status === "ready" && (
          <TocEntryList entries={toc.entries} campaignId={campaignId} depth={0} />
        )}
      </nav>
    </aside>
  );
}

function TocEntryList({
  entries,
  campaignId,
  depth,
}: {
  entries: readonly TocTreeEntry[];
  campaignId: string;
  depth: number;
}): React.ReactElement {
  return (
    <ul className="space-y-0.5" style={{ paddingLeft: depth > 0 ? "0.75rem" : 0 }}>
      {entries.map((entry) => (
        <TocEntryItem key={entry.treeId} entry={entry} campaignId={campaignId} depth={depth} />
      ))}
    </ul>
  );
}

function TocEntryItem({
  entry,
  campaignId,
  depth,
}: {
  entry: TocTreeEntry;
  campaignId: string;
  depth: number;
}): React.ReactElement {
  const params = useParams({ strict: false });
  const activeThingId = (params as Record<string, string | undefined>).thingId;
  const isActive = entry.thingId === activeThingId;
  const [deleting, setDeleting] = useState(false);

  async function handleDelete(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (deleting || !entry.thingId) return;
    setDeleting(true);
    try {
      await campaignClient.DELETE("/campaign/{id}/things/{thing_id}", {
        params: { path: { id: campaignId, thing_id: entry.thingId } },
      });
    } catch (err) {
      console.error("Failed to delete thing:", err);
    } finally {
      setDeleting(false);
    }
  }

  if (entry.kind !== "thing" || !entry.thingId) {
    return (
      <li>
        <span className="block rounded px-2 py-1 text-sm text-muted-foreground">
          {entry.title}
        </span>
        {entry.children.length > 0 && (
          <TocEntryList entries={entry.children} campaignId={campaignId} depth={depth + 1} />
        )}
      </li>
    );
  }

  return (
    <li>
      <Link
        to="/c/$campaignId/t/$thingId"
        params={{ campaignId: campaignId as CampaignId, thingId: entry.thingId as ThingId }}
        className={`group flex items-center justify-between rounded px-2 py-1 text-sm transition-colors ${
          isActive
            ? "bg-primary/10 text-foreground font-medium"
            : "text-foreground/80 hover:bg-foreground/5"
        }`}
      >
        <span className="truncate">{entry.title}</span>
        <button
          type="button"
          onClick={(e) => void handleDelete(e)}
          disabled={deleting}
          className="ml-1 hidden h-5 w-5 shrink-0 items-center justify-center rounded text-muted-foreground/60 transition-colors hover:bg-foreground/10 hover:text-foreground group-hover:flex disabled:opacity-50"
          aria-label={`Delete ${entry.title}`}
        >
          &times;
        </button>
      </Link>
      {entry.children.length > 0 && (
        <TocEntryList entries={entry.children} campaignId={campaignId} depth={depth + 1} />
      )}
    </li>
  );
}
