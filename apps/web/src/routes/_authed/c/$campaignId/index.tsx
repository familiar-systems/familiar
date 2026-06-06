// Index route for an authenticated campaign at /c/$campaignId. Loads campaign
// metadata, then branches three ways:
//   - onboarding unfinished       -> the creation wizard
//   - onboarded + home page set    -> redirect to the home-page editor
//   - onboarded + no home page set -> a placeholder (the home Page was
//     deleted; the FK clears the pointer. Choosing a new one is a future
//     feature, not built here.)

import type { CampaignMetadataResponse } from "@familiar-systems/types-campaign";
import { Navigate, createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { CampaignWizard } from "../../../../features/onboarding/CampaignWizard";
import { client } from "../../../../lib/api";
import { campaignClient } from "../../../../lib/campaigns-api";

interface LoadState {
  campaign: CampaignMetadataResponse | null;
  error: string | null;
}

function CampaignIndex(): React.ReactElement {
  const { campaignId } = Route.useParams();
  const [load, setLoad] = useState<LoadState>({ campaign: null, error: null });
  const [refetchKey, setRefetchKey] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const { response: leaseResp } = await client.GET("/campaigns/{id}", {
        params: { path: { id: campaignId as string } },
      });
      if (cancelled) return;
      if (!leaseResp.ok) {
        setLoad({ campaign: null, error: `Failed to load campaign (${leaseResp.status})` });
        return;
      }

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

  // Onboarding still in progress: show the wizard.
  if (load.campaign.wizard_completed_at === null) {
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

  // Onboarded: open the home-page editor.
  if (load.campaign.home_page_id !== null) {
    return (
      <Navigate
        to="/c/$campaignId/p/$pageId"
        params={{ campaignId, pageId: load.campaign.home_page_id }}
        replace
      />
    );
  }

  // Onboarded, but the home page pointer is null (the home Page was deleted).
  return (
    <section
      className="mx-auto w-full max-w-3xl space-y-3 px-8 pt-24"
      data-testid="campaign-no-home"
    >
      <h1 className="font-display text-3xl font-medium tracking-tight">{load.campaign.name}</h1>
      <p className="text-sm text-muted-foreground">
        This campaign has no home page. Choosing a new one is coming soon.
      </p>
    </section>
  );
}

export const Route = createFileRoute("/_authed/c/$campaignId/")({
  component: CampaignIndex,
});
