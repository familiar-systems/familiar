// Layout route for /c/$campaignId. Owns the `campaignId` param parsing/branding
// for every child (the index at /c/$campaignId and the editor at
// /c/$campaignId/t/$thingId) and renders them through the Outlet. The app
// chrome (Shell, backdrop) is provided one level up by _authed.tsx.

import type { CampaignId } from "@familiar-systems/types-app";
import { campaignIdSchema } from "@familiar-systems/types-app";
import { Outlet, createFileRoute, useParams } from "@tanstack/react-router";

import { LoroManagerProvider } from "../../../features/editor/LoroManagerProvider";
import { TocSidebar } from "../../../features/toc/TocSidebar";
import { useTocRoom } from "../../../features/toc/useToc";

function CampaignLayout(): React.ReactElement {
  const { campaignId } = Route.useParams();
  // One Loro WebSocket per campaign, shared by the ToC sidebar and every child
  // editor. Keyed by campaignId so switching campaigns rebuilds the manager on the
  // new socket URL (TanStack re-renders rather than remounts a same-route component
  // on a param change, so the key is what forces a fresh manager).
  return (
    <LoroManagerProvider key={campaignId} campaignId={campaignId}>
      <CampaignWorkspace campaignId={campaignId} />
    </LoroManagerProvider>
  );
}

// The sidebar is page-editing chrome, so it is shown only on a page route
// (`thingId` present), never over the onboarding wizard or the index redirect.
// It persists across navigation between pages because it lives here at the
// campaign layout, beside the Outlet. `min-h-0` lets both the sidebar and the
// editor pane scroll independently within the flex column the app Shell provides
// one level up.
function CampaignWorkspace({ campaignId }: { campaignId: CampaignId }): React.ReactElement {
  // Pin the ToC room for the whole workspace lifetime, above the page/index
  // branch so navigating between them never tears it down (the debounced leave
  // would absorb a quick toggle, but pinning avoids the churn entirely).
  useTocRoom();
  const onPage = useParams({ strict: false }).thingId !== undefined;
  if (!onPage) {
    return <Outlet />;
  }
  return (
    <div className="flex min-h-0 flex-1">
      <TocSidebar campaignId={campaignId} />
      <div className="min-w-0 flex-1 overflow-auto">
        <Outlet />
      </div>
    </div>
  );
}

export const Route = createFileRoute("/_authed/c/$campaignId")({
  parseParams: ({ campaignId }) => ({ campaignId: campaignIdSchema.parse(campaignId) }),
  component: CampaignLayout,
});
