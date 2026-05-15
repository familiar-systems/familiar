// Campaign route. v0 thin slice: only shows the wizard. Once the wizard
// seals (or fails), the user navigates back to the hub via the wizard's
// own "Back to hub" affordance.

import { campaignIdSchema, type Campaign } from "@familiar-systems/types-app";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { CampaignWizard } from "../../../features/onboarding/CampaignWizard";
import { client } from "../../../lib/api";

interface LoadState {
  campaign: Campaign | null;
  error: string | null;
}

function CampaignPage(): React.ReactElement {
  const { campaignId } = Route.useParams();
  const navigate = useNavigate();
  const [load, setLoad] = useState<LoadState>({ campaign: null, error: null });

  // Look up the campaign in the user's list. The list endpoint already
  // returns enough metadata (`name`, `tagline`, `wizard_completed_at`,
  // `last_init_error`) to decide whether to render the wizard. A future
  // slice will add `GET /api/campaigns/:id` for direct fetch.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const { data, response } = await client.GET("/campaigns");
      if (cancelled) return;
      if (!response.ok || !data) {
        setLoad({ campaign: null, error: `List failed (${response.status})` });
        return;
      }
      // Cross openapi-fetch's expanded-brand boundary back to the ts-rs alias
      // shape (see lib/auth.ts for the same pattern on MeResponse).
      const list = data as Campaign[];
      const found = list.find((c) => (c.id as string) === (campaignId as string));
      if (!found) {
        setLoad({ campaign: null, error: "Campaign not found." });
        return;
      }
      setLoad({ campaign: found, error: null });
    })();
    return () => {
      cancelled = true;
    };
  }, [campaignId]);

  if (load.error !== null) {
    return (
      <section className="mx-auto w-full max-w-3xl px-8 pt-24">
        <p className="text-sm text-muted-foreground">{load.error}</p>
      </section>
    );
  }
  if (load.campaign === null) {
    return (
      <section className="mx-auto w-full max-w-3xl px-8 pt-24">
        <p className="text-sm text-muted-foreground">Loading campaign...</p>
      </section>
    );
  }

  if (load.campaign.wizard_completed_at !== null) {
    // Sealed campaigns get a placeholder view — the next slice replaces
    // this with the real campaign editor.
    return (
      <section
        className="mx-auto w-full max-w-3xl space-y-4 px-8 pt-24"
        data-testid="campaign-placeholder"
      >
        <h1 className="font-display text-3xl font-medium tracking-tight">
          {load.campaign.name ?? "Untitled campaign"}
        </h1>
        <p className="text-sm text-muted-foreground">
          Sealed at {load.campaign.wizard_completed_at}. The campaign editor lands in the next
          slice.
        </p>
      </section>
    );
  }

  return (
    <section className="mx-auto w-full px-6 pt-12 pb-20">
      <CampaignWizard
        campaignId={campaignId as string}
        locale="en"
        onDone={() => {
          void navigate({ to: "/" });
        }}
      />
    </section>
  );
}

export const Route = createFileRoute("/_authed/c/$campaignId")({
  parseParams: ({ campaignId }) => ({ campaignId: campaignIdSchema.parse(campaignId) }),
  component: CampaignPage,
});
