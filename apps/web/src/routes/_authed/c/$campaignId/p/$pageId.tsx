// The home-page editor route: /c/$campaignId/p/$pageId. Parses/brands the
// page id and hands off to <HomeEditor>, which opens the CRDT-synced document
// and renders the TipTap editor.

import { pageIdSchema } from "@familiar-systems/types-campaign";
import { createFileRoute } from "@tanstack/react-router";

import { HomeEditor } from "../../../../../features/editor/HomeEditor";

function PageView(): React.ReactElement {
  const { pageId } = Route.useParams();
  return <HomeEditor pageId={pageId} />;
}

export const Route = createFileRoute("/_authed/c/$campaignId/p/$pageId")({
  parseParams: ({ pageId }) => ({ pageId: pageIdSchema.parse(pageId) }),
  component: PageView,
});
