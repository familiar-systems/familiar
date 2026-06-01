// Layout route for /c/$campaignId. Owns the `campaignId` param parsing/branding
// for every child (the index at /c/$campaignId and the editor at
// /c/$campaignId/t/$thingId) and renders them through the Outlet. The app
// chrome (Shell, backdrop) is provided one level up by _authed.tsx.

import { campaignIdSchema } from "@familiar-systems/types-app";
import { Outlet, createFileRoute } from "@tanstack/react-router";

function CampaignLayout(): React.ReactElement {
  return <Outlet />;
}

export const Route = createFileRoute("/_authed/c/$campaignId")({
  parseParams: ({ campaignId }) => ({ campaignId: campaignIdSchema.parse(campaignId) }),
  component: CampaignLayout,
});
