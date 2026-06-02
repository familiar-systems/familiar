// Layout route for /c/$campaignId. Owns the `campaignId` param parsing/branding
// for every child (the index at /c/$campaignId and the editor at
// /c/$campaignId/t/$thingId) and renders them through the Outlet. The app
// chrome (Shell, backdrop) is provided one level up by _authed.tsx.

import { campaignIdSchema } from "@familiar-systems/types-app";
import { Outlet, createFileRoute } from "@tanstack/react-router";

import { LoroManagerProvider } from "../../../features/editor/LoroManagerProvider";

function CampaignLayout(): React.ReactElement {
  const { campaignId } = Route.useParams();
  // One Loro WebSocket per campaign, shared by every child editor. Keyed by
  // campaignId so switching campaigns rebuilds the manager on the new socket URL
  // (TanStack re-renders rather than remounts a same-route component on a param
  // change, so the key is what forces a fresh manager).
  return (
    <LoroManagerProvider key={campaignId} campaignId={campaignId}>
      <Outlet />
    </LoroManagerProvider>
  );
}

export const Route = createFileRoute("/_authed/c/$campaignId")({
  parseParams: ({ campaignId }) => ({ campaignId: campaignIdSchema.parse(campaignId) }),
  component: CampaignLayout,
});
