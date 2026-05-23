import { campaignIdSchema } from "@familiar-systems/types-app";
import type { CampaignMetadataResponse } from "@familiar-systems/types-campaign";
import { Outlet, createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { CampaignWizard } from "../../../features/onboarding/CampaignWizard";
import { LoroManagerProvider } from "../../../features/campaign/LoroManagerProvider";
import { TocSidebar } from "../../../features/campaign/TocSidebar";
import { client } from "../../../lib/api";
import { campaignClient } from "../../../lib/campaigns-api";
import { campaignPath } from "../../../lib/paths";
import { getSessionToken } from "../../../lib/hanko";

interface LoadState {
  campaign: CampaignMetadataResponse | null;
  error: string | null;
}

function CampaignLayout(): React.ReactElement {
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
        setLoad({
          campaign: null,
          error: `Failed to load campaign (${leaseResp.status})`,
        });
        return;
      }

      const { data, response } = await campaignClient.GET("/campaign/{id}", {
        params: { path: { id: campaignId as string } },
      });
      if (cancelled) return;
      if (!response.ok || !data) {
        setLoad({
          campaign: null,
          error: `Failed to load campaign (${response.status})`,
        });
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

  const token = getSessionToken();
  const wsProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const wsUrl = `${wsProtocol}//${window.location.host}${campaignPath(`${campaignId as string}/ws`)}${token ? `?token=${token}` : ""}`;

  return (
    <LoroManagerProvider wsUrl={wsUrl}>
      <div className="flex h-full">
        <TocSidebar campaignId={campaignId as string} />
        <main className="flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>
    </LoroManagerProvider>
  );
}

export const Route = createFileRoute("/_authed/c/$campaignId")({
  parseParams: ({ campaignId }) => ({
    campaignId: campaignIdSchema.parse(campaignId),
  }),
  component: CampaignLayout,
});
