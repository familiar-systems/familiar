import { createFileRoute } from "@tanstack/react-router";

function CampaignHome(): React.ReactElement {
  return (
    <section className="flex h-full items-center justify-center">
      <div className="text-center">
        <p className="text-lg text-muted-foreground">
          Select a page from the sidebar, or create one with the + button.
        </p>
      </div>
    </section>
  );
}

export const Route = createFileRoute("/_authed/c/$campaignId/")({
  component: CampaignHome,
});
