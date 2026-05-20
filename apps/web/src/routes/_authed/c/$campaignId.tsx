// Campaign route. Fetches metadata directly from the campaign server to
// prove the full round-trip: create on the shard, read it back. After
// successful initialization the route refetches and transitions to the
// initialized campaign view.

import { campaignIdSchema } from "@familiar-systems/types-app";
import type { CampaignMetadataResponse } from "@familiar-systems/types-campaign";
import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { CampaignWizard } from "../../../features/onboarding/CampaignWizard";
import { campaignClient } from "../../../lib/campaigns-api";

interface LoadState {
  campaign: CampaignMetadataResponse | null;
  error: string | null;
}

function CampaignPage(): React.ReactElement {
  const { campaignId } = Route.useParams();
  const [load, setLoad] = useState<LoadState>({ campaign: null, error: null });
  const [refetchKey, setRefetchKey] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const { data, response } = await campaignClient.GET("/campaign/{id}", {
        params: { path: { id: campaignId as string } },
      });
      if (cancelled) return;
      if (!response.ok || !data) {
        setLoad({ campaign: null, error: `Failed to load campaign (${response.status})` });
        return;
      }
      setLoad({ campaign: data as CampaignMetadataResponse, error: null });
    })();
    return () => {
      cancelled = true;
    };
  }, [campaignId, refetchKey]);

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
    return (
      <section
        className="mx-auto w-full max-w-3xl space-y-4 px-8 pt-24"
        data-testid="campaign-placeholder"
      >
        <h1 className="font-display text-3xl font-medium tracking-tight">{load.campaign.name}</h1>
        <p className="text-sm text-muted-foreground">
          Initialized at {load.campaign.wizard_completed_at}. The campaign editor lands in the next
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
          setRefetchKey((k) => k + 1);
        }}
      />
    </section>
  );
}

export const Route = createFileRoute("/_authed/c/$campaignId")({
  parseParams: ({ campaignId }) => ({ campaignId: campaignIdSchema.parse(campaignId) }),
  component: CampaignPage,
});
