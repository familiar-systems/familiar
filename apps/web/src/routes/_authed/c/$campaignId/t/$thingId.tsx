// The home-page editor route: /c/$campaignId/t/$thingId. Parses/brands the
// thing id and hands off to <HomeEditor>, which opens the CRDT-synced document
// and renders the TipTap editor.

import { thingIdSchema } from "@familiar-systems/types-campaign";
import { createFileRoute } from "@tanstack/react-router";

import { HomeEditor } from "../../../../../features/editor/HomeEditor";

function ThingPage(): React.ReactElement {
  const { thingId } = Route.useParams();
  return <HomeEditor thingId={thingId} />;
}

export const Route = createFileRoute("/_authed/c/$campaignId/t/$thingId")({
  parseParams: ({ thingId }) => ({ thingId: thingIdSchema.parse(thingId) }),
  component: ThingPage,
});
